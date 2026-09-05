//
// Copyright (c) 2026 Wing Mun Fung
//
// This program and the accompanying materials are made available under the
// terms of the Eclipse Public License 2.0, available at
// https://www.eclipse.org/legal/epl-2.0/, or the Apache License, Version 2.0,
// available at https://www.apache.org/licenses/LICENSE-2.0.
//
// SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
//

use anyhow::Result;
use rosbridge_server_rs::{
    backend::*,
    bridge::Bridge,
    wire::{self, Compression, Decoder, Options},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Default)]
struct TestRos {
    next: u64,
    entities: HashMap<Entity, (String, String, &'static str)>,
    events: Vec<Event>,
    writes: Vec<(Entity, Value)>,
    qos: Vec<Qos>,
    discarded: Vec<(Entity, u64)>,
}

impl TestRos {
    fn create(&mut self, n: &str, t: &str, kind: &'static str) -> Result<Entity> {
        self.next += 1;
        self.entities.insert(self.next, (n.into(), t.into(), kind));
        Ok(self.next)
    }
    fn entity(&self, kind: &str) -> Entity {
        *self
            .entities
            .iter()
            .find(|(_, (_, _, k))| *k == kind)
            .unwrap()
            .0
    }
    fn message(&mut self, v: Value) {
        let e = self.entity("subscription");
        self.events.push(Event::Message(
            e,
            RosMessage {
                cbor: wire::cbor_value(&v),
                json: v,
                raw: vec![],
                stamp: (0, 0),
            },
        ));
    }
}

impl Backend for TestRos {
    fn publisher(&mut self, n: &str, t: &str, q: Qos) -> Result<Entity> {
        self.qos.push(q);
        self.create(n, t, "publisher")
    }
    fn subscription(&mut self, n: &str, t: &str, q: Qos) -> Result<Entity> {
        self.qos.push(q);
        self.create(n, t, "subscription")
    }
    fn client(&mut self, n: &str, t: &str) -> Result<Entity> {
        self.create(n, t, "client")
    }
    fn service(&mut self, n: &str, t: &str) -> Result<Entity> {
        self.create(n, t, "service")
    }
    fn publish(&mut self, e: Entity, v: &Value) -> Result<()> {
        self.writes.push((e, v.clone()));
        Ok(())
    }
    fn request(&mut self, e: Entity, v: &Value) -> Result<i64> {
        self.writes.push((e, v.clone()));
        Ok(1)
    }
    fn validate_response(&self, _e: Entity, _v: &Value) -> Result<()> {
        Ok(())
    }
    fn respond(&mut self, e: Entity, _r: u64, v: &Value) -> Result<()> {
        self.writes.push((e, v.clone()));
        Ok(())
    }
    fn discard_request(&mut self, e: Entity, r: u64) {
        self.discarded.push((e, r));
    }
    fn destroy(&mut self, e: Entity) {
        self.entities.remove(&e);
    }
    fn poll(&mut self) -> Result<Vec<Event>> {
        Ok(std::mem::take(&mut self.events))
    }
    fn topics(&mut self) -> Result<BTreeMap<String, Vec<String>>> {
        Ok(self
            .entities
            .values()
            .filter(|(_, _, k)| *k == "publisher" || *k == "subscription")
            .map(|(n, t, _)| (n.clone(), vec![t.clone()]))
            .collect())
    }
    fn services(&mut self) -> Result<BTreeMap<String, Vec<String>>> {
        Ok(BTreeMap::from([(
            "/add".into(),
            vec!["example_interfaces/srv/AddTwoInts".into()],
        )]))
    }
    fn publisher_qos(&mut self, _: &str) -> Result<Vec<Qos>> {
        Ok(vec![])
    }
    fn now(&self) -> (i64, u32) {
        (10, 20)
    }
}

fn setup() -> (Bridge<TestRos>, mpsc::Receiver<Vec<Message>>) {
    let mut b = Bridge::new(TestRos::default(), Duration::from_secs(1));
    let (tx, rx) = mpsc::channel(64);
    b.connect(1, tx);
    (b, rx)
}

fn receive(rx: &mut mpsc::Receiver<Vec<Message>>) -> Value {
    let frames = rx.try_recv().unwrap();
    let Message::Text(text) = &frames[0] else {
        panic!("expected JSON")
    };
    serde_json::from_str(text).unwrap()
}

#[test]
fn event_wait_respects_service_deadlines() {
    let (mut bridge, _rx) = setup();
    assert_eq!(bridge.next_wakeup(), Duration::from_millis(100));
    bridge.command(1, json!({"op":"call_service","service":"/deadline_test","type":"example_interfaces/AddTwoInts","args":{},"timeout":0.01}));
    assert!(bridge.next_wakeup() <= Duration::from_millis(10));
    bridge.disconnect(1);
    assert_eq!(bridge.next_wakeup(), Duration::from_millis(100));
}

#[test]
fn advertisement_ids_and_disconnect_release_only_last_owner() {
    let (mut b, _) = setup();
    let (tx, _rx) = mpsc::channel(64);
    b.connect(2, tx);
    for (owner, id) in [(1, "a"), (1, "b"), (2, "c")] {
        b.command(
            owner,
            json!({"op":"advertise","id":id,"topic":"/x","type":"std_msgs/String"}),
        );
    }
    assert_eq!(b.backend.entities.len(), 1);
    b.command(1, json!({"op":"unadvertise","id":"a","topic":"/x"}));
    assert_eq!(b.backend.entities.len(), 1);
    b.disconnect(1);
    assert_eq!(b.backend.entities.len(), 1);
    b.disconnect(2);
    assert!(b.backend.entities.is_empty());
}

#[test]
fn subscriptions_are_coalesced_per_connection() {
    let (mut b, mut rx) = setup();
    for id in ["a", "b"] {
        b.command(
            1,
            json!({"op":"subscribe","id":id,"topic":"/x","type":"std_msgs/String"}),
        );
    }
    b.backend.message(json!({"data":"one"}));
    b.tick().unwrap();
    assert_eq!(receive(&mut rx)["msg"]["data"], "one");
    assert!(rx.try_recv().is_err());
    b.command(1, json!({"op":"unsubscribe","id":"a","topic":"/x"}));
    assert_eq!(b.backend.entities.len(), 1);
    b.command(1, json!({"op":"unsubscribe","id":"b","topic":"/x"}));
    assert!(b.backend.entities.is_empty());
}

#[test]
fn first_publisher_qos_wins() {
    let (mut b, _) = setup();
    b.command(
        1,
        json!({"op":"advertise","topic":"/x","type":"std_msgs/String","qos":{"depth":3}}),
    );
    b.command(
        1,
        json!({"op":"advertise","topic":"/x","type":"std_msgs/String","qos":{"depth":20}}),
    );
    assert_eq!(b.backend.qos.len(), 1);
    assert_eq!(b.backend.qos[0].depth, 3);
}

#[test]
fn conflicting_type_is_rejected_without_changing_publisher() {
    let (mut b, mut rx) = setup();
    b.command(
        1,
        json!({"op":"advertise","topic":"/x","type":"std_msgs/String"}),
    );
    b.command(
        1,
        json!({"op":"advertise","id":"bad","topic":"/x","type":"std_msgs/Int32"}),
    );
    assert_eq!(receive(&mut rx)["id"], "bad");
    assert_eq!(b.backend.entities.len(), 1);
}

#[test]
fn response_ids_correlate_and_clients_are_destroyed() {
    let (mut b, mut rx) = setup();
    for id in ["first", "second"] {
        b.command(
            1,
            json!({"op":"call_service","id":id,"service":"/add","args":{"a":1,"b":2}}),
        );
    }
    let ids: Vec<_> = b.backend.writes.iter().map(|(e, _)| *e).collect();
    b.backend.events.push(Event::Response {
        entity: ids[1],
        sequence: 1,
        values: json!({"sum":3}),
    });
    b.tick().unwrap();
    assert_eq!(receive(&mut rx)["id"], "second");
    b.backend.events.push(Event::Response {
        entity: ids[0],
        sequence: 1,
        values: json!({"sum":3}),
    });
    b.tick().unwrap();
    assert_eq!(receive(&mut rx)["id"], "first");
    assert!(b.backend.entities.is_empty());
}

#[test]
fn service_timeout_returns_failure_and_cleans_up() {
    let (mut b, mut rx) = setup();
    b.command(
        1,
        json!({"op":"call_service","id":"t","service":"/add","timeout":0}),
    );
    b.tick().unwrap();
    let v = receive(&mut rx);
    assert_eq!(v["result"], false);
    assert_eq!(v["id"], "t");
    assert!(b.backend.entities.is_empty());
}

#[test]
fn client_cannot_spoof_other_clients_service_response() {
    let (mut b, mut rx) = setup();
    let (tx, mut attacker) = mpsc::channel(64);
    b.connect(2, tx);
    b.command(
        1,
        json!({"op":"advertise_service","service":"/add","type":"example_interfaces/AddTwoInts"}),
    );
    let e = b.backend.entity("service");
    b.backend.events.push(Event::Request {
        entity: e,
        request: 9,
        args: json!({"a":1,"b":2}),
    });
    b.tick().unwrap();
    let request = receive(&mut rx);
    b.command(2,json!({"op":"service_response","id":request["id"],"service":"/add","result":true,"values":{"sum":99}}));
    assert_eq!(receive(&mut attacker)["level"], "error");
    assert!(b.backend.writes.is_empty());
    b.command(1,json!({"op":"service_response","id":request["id"],"service":"/add","result":true,"values":{"sum":3}}));
    assert_eq!(b.backend.writes[0].1["sum"], 3);
}

#[test]
fn stalled_client_is_disconnected_and_entities_removed() {
    let (mut b, _) = setup();
    let (tx, _rx) = mpsc::channel(1);
    b.connect(1, tx);
    b.command(
        1,
        json!({"op":"subscribe","topic":"/x","type":"std_msgs/String"}),
    );
    b.backend.message(json!({"data":"one"}));
    b.backend.message(json!({"data":"two"}));
    b.tick().unwrap();
    assert!(b.backend.entities.is_empty());
}

#[test]
fn qos_defaults_and_durations() {
    assert_eq!(Qos::parse(&json!({})).unwrap(), Qos::default());
    let q=Qos::parse(&json!({"deadline":1.25,"lifespan":{"secs":2,"nsecs":1500000000},"reliability":"BEST_EFFORT"})).unwrap();
    assert_eq!(q.deadline, (1, 250000000));
    assert_eq!(q.lifespan, (3, 500000000));
    assert_eq!(q.reliability, 2);
    for v in [
        json!({"depth":-1}),
        json!({"deadline":true}),
        json!({"history":"random"}),
    ] {
        assert!(Qos::parse(&v).is_err());
    }
}

#[test]
fn interface_names_are_normalized() {
    assert_eq!(
        type_name("std_msgs/String", "msg").unwrap(),
        "std_msgs/msg/String"
    );
    assert_eq!(
        type_name("test_msgs/action/Fibonacci", "action").unwrap(),
        "test_msgs/action/Fibonacci"
    );
    assert!(type_name("test_msgs/srv/Foo", "msg").is_err());
    assert!(type_name("../Foo", "msg").is_err());
}

#[test]
fn legacy_rosapi_service_names_are_normalized() {
    for name in [
        "rosapi/TopicsAndRawTypes",
        "rosapi/srv/TopicsAndRawTypes",
        "rosapi_msgs/srv/TopicsAndRawTypes",
    ] {
        assert_eq!(
            type_name(name, "srv").unwrap(),
            "rosapi_msgs/srv/TopicsAndRawTypes"
        );
    }
    assert_eq!(type_name("rosapi/Foo", "msg").unwrap(), "rosapi/msg/Foo");
    assert_eq!(type_name("custom/Foo", "srv").unwrap(), "custom/srv/Foo");
}

#[test]
fn unicode_fragments_roundtrip_out_of_order() {
    let value = json!({"op":"publish","topic":"/x","msg":{"data":"中文🦀"}});
    let options = Options {
        fragment: Some(8),
        ..Default::default()
    };
    let mut frames = wire::encode(&value, None, &options, "f").unwrap();
    frames.reverse();
    let mut decoder = Decoder::default();
    let mut result = None;
    for f in frames {
        if let Some(v) = decoder.decode(f, 1024).unwrap() {
            result = Some(v);
        }
    }
    assert_eq!(result, Some(value));
}

#[test]
fn fragmented_messages_enforce_aggregate_limit() {
    let mut d = Decoder::default();
    let f = |n, data| {
        Message::Text(json!({"op":"fragment","id":"f","num":n,"total":2,"data":data}).to_string())
    };
    let payload = "x".repeat(180);
    assert!(d.decode(f(0, &payload), 300).unwrap().is_none());
    assert!(d.decode(f(1, &payload), 300).is_err());
}

#[test]
fn cbor_typed_array_decodes_to_json() {
    let tag = ciborium::Value::Tag(
        78,
        Box::new(ciborium::Value::Bytes(
            [1i32.to_le_bytes(), (-2i32).to_le_bytes()].concat(),
        )),
    );
    assert_eq!(wire::json_value(tag).unwrap(), json!([1, -2]));
}

#[test]
fn png_roundtrips_utf8() {
    use base64::Engine;
    let v = json!({"op":"publish","msg":{"data":"中文"}});
    let frames = wire::encode(
        &v,
        None,
        &Options {
            compression: Compression::Png,
            ..Default::default()
        },
        "p",
    )
    .unwrap();
    let Message::Text(s) = &frames[0] else {
        panic!()
    };
    let envelope: Value = serde_json::from_str(s).unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(envelope["data"].as_str().unwrap())
        .unwrap();
    let mut reader = png::Decoder::new(bytes.as_slice()).read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).unwrap();
    let decoded: Value = serde_json::from_slice(&buffer[..info.buffer_size()]).unwrap();
    assert_eq!(decoded, v);
}

#[test]
fn absent_action_server_times_out_and_releases_entities() {
    let mut bridge = Bridge::new(TestRos::default(), Duration::ZERO);
    let (tx, mut rx) = mpsc::channel(64);
    bridge.connect(1, tx);
    bridge.command(1, json!({"op":"send_action_goal", "id":"goal", "action":"/missing", "action_type":"example_interfaces/action/Fibonacci", "args":{"order":3}}));
    assert_eq!(bridge.backend.entities.len(), 5);
    bridge.tick().unwrap();
    let result = receive(&mut rx);
    assert_eq!(result["op"], "action_result");
    assert_eq!(result["id"], "goal");
    assert_eq!(result["result"], false);
    assert!(bridge.backend.entities.is_empty());
}

#[test]
fn malformed_action_request_does_not_stop_other_clients() {
    let (mut bridge, _rx) = setup();
    bridge.command(
        1,
        json!({"op":"advertise_action", "action":"/action", "type":"test_msgs/Fibonacci"}),
    );
    let entity = *bridge
        .backend
        .entities
        .iter()
        .find(|(_, (name, _, _))| name.ends_with("/send_goal"))
        .unwrap()
        .0;
    bridge.backend.events.push(Event::Request {
        entity,
        request: 1,
        args: json!({"goal_id":{"uuid":[]}, "goal":{}}),
    });
    bridge.tick().unwrap();
    assert_eq!(bridge.backend.discarded, vec![(entity, 1)]);
    bridge.command(
        1,
        json!({"op":"advertise", "topic":"/still_alive", "type":"std_msgs/String"}),
    );
    assert!(
        bridge
            .backend
            .entities
            .values()
            .any(|(name, _, _)| name == "/still_alive")
    );
}

#[test]
fn transient_local_sample_reaches_late_websocket_client() {
    let (mut bridge, mut first) = setup();
    bridge.command(1, json!({"op":"subscribe", "topic":"/retained", "type":"std_msgs/String", "qos":{"durability":"transient_local"}}));
    bridge.backend.message(json!({"data":"retained"}));
    bridge.tick().unwrap();
    assert_eq!(receive(&mut first)["msg"]["data"], "retained");
    let (sender, mut second) = mpsc::channel(64);
    bridge.connect(2, sender);
    bridge.command(
        2,
        json!({"op":"subscribe", "topic":"/retained", "type":"std_msgs/String"}),
    );
    assert_eq!(receive(&mut second)["msg"]["data"], "retained");
}

#[test]
fn service_request_ids_match_python_and_reset_on_readvertise() {
    let (mut bridge, mut output) = setup();
    for _ in 0..2 {
        bridge.command(1, json!({"op":"advertise_service","service":"/add","type":"example_interfaces/AddTwoInts"}));
        let entity = bridge.backend.entity("service");
        for request in 1..=2 {
            bridge.backend.events.push(Event::Request {
                entity,
                request,
                args: json!({"a":1,"b":2}),
            });
            bridge.tick().unwrap();
            assert_eq!(
                receive(&mut output)["id"],
                format!("service_request:/add:{request}")
            );
        }
        bridge.command(1, json!({"op":"unadvertise_service","service":"/add"}));
    }
}

#[test]
fn shared_subscription_uses_python_compression_precedence() {
    let (mut bridge, mut output) = setup();
    for (id, compression) in [("plain", "none"), ("binary", "cbor")] {
        bridge.command(1,json!({"op":"subscribe","id":id,"topic":"/x","type":"std_msgs/String","compression":compression,"fragment_size":1}));
    }
    bridge.backend.message(json!({"data":"hello"}));
    bridge.tick().unwrap();
    let frames = output.try_recv().unwrap();
    assert_eq!(frames.len(), 1);
    assert!(matches!(frames[0], Message::Binary(_)));
    bridge.command(1, json!({"op":"unsubscribe","id":"binary","topic":"/x"}));
    bridge.backend.message(json!({"data":"hello"}));
    bridge.tick().unwrap();
    let frames = output.try_recv().unwrap();
    assert!(frames.len() > 1);
    assert!(matches!(frames[0], Message::Text(_)));
}

fn restricted(settings: &[&str]) -> (Bridge<TestRos>, mpsc::Receiver<Vec<Message>>) {
    let (mut bridge, rx) = setup();
    let args: Vec<String> = settings
        .iter()
        .flat_map(|v| ["-p".to_owned(), (*v).to_owned()])
        .collect();
    bridge.access = rosbridge_server_rs::access::Access::from_ros_args(&args).unwrap();
    (bridge, rx)
}

#[test]
fn forwarding_allowlists_reject_before_creating_entities() {
    let (mut bridge, mut rx) = restricted(&[
        "topics_glob:=[]",
        "topics_pub_glob:=[/write/*]",
        "topics_sub_glob:=[/read/*]",
        "services_glob:=[]",
    ]);
    for request in [
        json!({"op":"advertise","topic":"/read/imu","type":"std_msgs/String"}),
        json!({"op":"publish","topic":"read/imu","type":"std_msgs/String","msg":{"data":"x"}}),
        json!({"op":"subscribe","topic":"/write/cmd","type":"std_msgs/String"}),
        json!({"op":"call_service","service":"/hidden","type":"std_srvs/Trigger"}),
        json!({"op":"advertise_service","service":"/hidden","type":"std_srvs/Trigger"}),
        json!({"op":"send_action_goal","action":"/move","type":"example_interfaces/Fibonacci"}),
        json!({"op":"advertise_action","action":"/move","type":"example_interfaces/Fibonacci"}),
    ] {
        bridge.command(1, request);
        let response = receive(&mut rx);
        assert!(response.to_string().contains("denies"), "{response}");
        assert!(bridge.backend.entities.is_empty());
        assert!(bridge.backend.writes.is_empty());
    }
    bridge.command(
        1,
        json!({"op":"subscribe","topic":"/read/imu","type":"std_msgs/String"}),
    );
    bridge.command(
        1,
        json!({"op":"publish","topic":"/write/cmd","type":"std_msgs/String","msg":{"data":"ok"}}),
    );
    assert_eq!(bridge.backend.entities.len(), 2);
    assert_eq!(bridge.backend.writes.len(), 1);
    bridge.disconnect(1);
    assert!(bridge.backend.entities.is_empty());
}

#[test]
fn legacy_topics_union_and_explicit_ros_override() {
    let (mut bridge, mut rx) = restricted(&[
        "topics_glob:=['/shared/*']",
        "topics_pub_glob:=[]",
        "topics_sub_glob:=['/old/*']",
        "topics_sub_glob:=['/read/*']",
    ]);
    for topic in ["/shared/data", "/read/data"] {
        bridge.command(
            1,
            json!({"op":"subscribe","topic":topic,"type":"std_msgs/String"}),
        );
    }
    bridge.command(
        1,
        json!({"op":"advertise","topic":"/shared/data","type":"std_msgs/String"}),
    );
    assert_eq!(bridge.backend.entities.len(), 3);
    bridge.command(
        1,
        json!({"op":"subscribe","topic":"/old/data","type":"std_msgs/String"}),
    );
    assert!(receive(&mut rx).to_string().contains("denies"));
    assert_eq!(bridge.backend.entities.len(), 3);
}

#[test]
fn action_topics_cannot_bypass_topic_allowlists() {
    let (mut bridge, mut rx) = restricted(&["topics_glob:=[]"]);
    for op in ["send_action_goal", "advertise_action"] {
        bridge.command(
            1,
            json!({"op":op,"action":"/move","type":"example_interfaces/Fibonacci"}),
        );
        assert!(receive(&mut rx).to_string().contains("denies"));
        assert!(bridge.backend.entities.is_empty());
    }
}

#[test]
fn parameter_allowlist_blocks_raw_services_and_filters_names() {
    let (mut bridge, mut rx) = restricted(&["params_glob:=['public_*']"]);
    for (service, typ, args) in [
        (
            "/rosapi/get_param",
            "rosapi_msgs/GetParam",
            json!({"name":"/node:secret"}),
        ),
        (
            "/node/get_parameters",
            "rcl_interfaces/GetParameters",
            json!({"names":["secret"]}),
        ),
        (
            "/alias",
            "rcl_interfaces/SetParameters",
            json!({"parameters":[]}),
        ),
    ] {
        bridge.command(
            1,
            json!({"op":"call_service","service":service,"type":typ,"args":args}),
        );
        assert_eq!(receive(&mut rx)["result"], false);
        assert!(bridge.backend.entities.is_empty());
    }
    bridge.command(1, json!({"op":"call_service","service":"/rosapi/get_param_names","type":"rosapi_msgs/GetParamNames","args":{}}));
    let entity = bridge.backend.entity("client");
    bridge.backend.events.push(Event::Response {
        entity,
        sequence: 1,
        values: json!({"names":["/node:secret","/node:public_rate"]}),
    });
    bridge.tick().unwrap();
    assert_eq!(
        receive(&mut rx)["values"]["names"],
        json!(["/node:public_rate"])
    );
    bridge.command(1, json!({"op":"call_service","service":"/rosapi/get_param","type":"rosapi_msgs/GetParam","args":{"name":"/node:public_rate"}}));
    assert_eq!(bridge.backend.entities.len(), 1);
}
