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

//! Cases ported from rosbridge_suite test/internal/test_message_conversion.py.
//! See the root LICENSE file for upstream attribution.
use super::message::MessageType;
use serde_json::{Value, json};

fn roundtrip(typ: &str, value: Value) {
    let typ = MessageType::load(typ).unwrap();
    for value in [
        value.clone(),
        serde_json::from_str(&value.to_string()).unwrap(),
    ] {
        let mut message = typ.message();
        message.fill(&value, (123, 456)).unwrap();
        assert_eq!(message.values().unwrap().0, value);
        assert!(!message.raw().unwrap().is_empty());
    }
}

#[test]
fn signed_int_base_msgs() {
    for bits in [8, 16, 32, 64] {
        let typ = format!("std_msgs/msg/Int{bits}");
        for value in -127..128 {
            roundtrip(&typ, json!({"data":value}));
        }
        let max = (1_i128 << (bits - 1)) - 1;
        for value in [-max - 1, -max, max] {
            roundtrip(&typ, json!({"data":value as i64}));
        }
        let schema = MessageType::load(&typ).unwrap();
        assert!(
            schema
                .message()
                .fill(&json!({"data":max as u64 + 1}), (0, 0))
                .is_err()
        );
    }
}

#[test]
fn unsigned_int_base_msgs() {
    for bits in [8, 16, 32, 64] {
        let typ = format!("std_msgs/msg/UInt{bits}");
        for value in 0..256 {
            roundtrip(&typ, json!({"data":value}));
        }
        let max = ((1_u128 << bits) - 1) as u64;
        roundtrip(&typ, json!({"data":max}));
        let schema = MessageType::load(&typ).unwrap();
        assert!(schema.message().fill(&json!({"data":-1}), (0, 0)).is_err());
        if bits < 64 {
            assert!(
                schema
                    .message()
                    .fill(&json!({"data":max+1}), (0, 0))
                    .is_err()
            );
        }
    }
}

#[test]
fn byte_and_char_base_msgs() {
    for typ in ["std_msgs/msg/Byte", "std_msgs/msg/Char"] {
        for value in 0..256 {
            roundtrip(typ, json!({"data":value}));
        }
    }
}

#[test]
fn bool_base_msg() {
    for value in [true, false] {
        roundtrip("std_msgs/msg/Bool", json!({"data":value}));
    }
}

#[test]
fn string_base_msg() {
    for value in [
        "",
        "bool",
        "int8",
        "uint64",
        "float32",
        "string",
        "hello 世界",
    ] {
        roundtrip("std_msgs/msg/String", json!({"data":value}));
    }
}

#[test]
fn time_and_duration_msgs() {
    for typ in [
        "builtin_interfaces/msg/Time",
        "builtin_interfaces/msg/Duration",
    ] {
        roundtrip(typ, json!({"sec":3,"nanosec":5}));
    }
}

#[test]
fn header_msg() {
    roundtrip(
        "std_msgs/msg/Header",
        json!({"stamp":{"sec":12347,"nanosec":322304},"frame_id":"2394dnfnlcx;v[p234j]"}),
    );
}

#[test]
fn assorted_default_msgs() {
    for typ in [
        "geometry_msgs/msg/Pose",
        "action_msgs/msg/GoalStatus",
        "geometry_msgs/msg/WrenchStamped",
        "stereo_msgs/msg/DisparityImage",
        "nav_msgs/msg/OccupancyGrid",
        "geometry_msgs/msg/Point32",
        "std_msgs/msg/String",
        "trajectory_msgs/msg/JointTrajectoryPoint",
        "diagnostic_msgs/msg/KeyValue",
        "visualization_msgs/msg/InteractiveMarkerUpdate",
        "nav_msgs/msg/GridCells",
        "sensor_msgs/msg/PointCloud2",
    ] {
        let schema = MessageType::load(typ).unwrap();
        roundtrip(typ, schema.message().values().unwrap().0);
    }
}

#[test]
fn uint8array_list_and_base64() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let data: Vec<u8> = (0..=255).collect();
    let encoded = STANDARD.encode(&data);
    let typ = MessageType::load("std_msgs/msg/UInt8MultiArray").unwrap();
    for input in [json!(data), json!(encoded)] {
        let mut message = typ.message();
        message.fill(&json!({"data":input}), (0, 0)).unwrap();
        assert_eq!(message.values().unwrap().0["data"], encoded);
    }
}

#[test]
fn float32array_integer_and_float_input() {
    let schema = MessageType::load("std_msgs/msg/Float32MultiArray").unwrap();
    let integers: Vec<i32> = (0..256).collect();
    let floats: Vec<f32> = (0..256).map(|x| x as f32).collect();
    for input in [json!(integers), json!(floats)] {
        let mut message = schema.message();
        message.fill(&json!({"data":input}), (0, 0)).unwrap();
        assert_eq!(message.values().unwrap().0["data"], json!(floats));
    }
}

#[test]
fn upstream_custom_messages() {
    for typ in ["TestTimeArray", "TestDurationArray"] {
        let field = if typ == "TestTimeArray" {
            "times"
        } else {
            "durations"
        };
        roundtrip(
            &format!("rosbridge_test_msgs/msg/{typ}"),
            json!({field:[{"sec":3,"nanosec":5},{"sec":2,"nanosec":7}]}),
        );
    }
    let header = json!({"stamp":{"sec":12347,"nanosec":322304},"frame_id":"2394dnfnlcx;v[p234j]"});
    for typ in ["TestHeader", "TestHeaderTwo"] {
        roundtrip(
            &format!("rosbridge_test_msgs/msg/{typ}"),
            json!({"header":header}),
        );
    }
    roundtrip(
        "rosbridge_test_msgs/msg/TestHeaderArray",
        json!({"header":[header,header,header]}),
    );
}

#[test]
fn upstream_byte_arrays() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    for (name, size) in [
        ("TestChar", 256),
        ("TestUInt8", 256),
        ("TestUInt8FixedSizeArray16", 16),
    ] {
        let schema = MessageType::load(&format!("rosbridge_test_msgs/msg/{name}")).unwrap();
        let bytes: Vec<u8> = (0..size).map(|x| x as u8).collect();
        let encoded = STANDARD.encode(&bytes);
        for input in [json!(bytes), json!(encoded)] {
            let mut message = schema.message();
            message.fill(&json!({"data":input}), (0, 0)).unwrap();
            assert_eq!(message.values().unwrap().0["data"], encoded);
        }
    }
}

#[test]
fn upstream_float_arrays() {
    for (name, size) in [("TestFloat32Array", 256), ("TestFloat32BoundedArray", 16)] {
        let schema = MessageType::load(&format!("rosbridge_test_msgs/msg/{name}")).unwrap();
        let integers: Vec<i32> = (0..size).collect();
        let floats: Vec<f32> = (0..size).map(|x| x as f32).collect();
        for input in [json!(integers), json!(floats)] {
            let mut message = schema.message();
            message.fill(&json!({"data":input}), (0, 0)).unwrap();
            assert_eq!(message.values().unwrap().0["data"], json!(floats));
        }
    }
    let floats: Vec<f32> = (0..16).map(|x| x as f32).collect();
    roundtrip(
        "rosbridge_test_msgs/msg/TestNestedBoundedArray",
        json!({"data":{"data":floats}}),
    );
}

#[test]
fn nonfinite_floats_extract_as_null() {
    for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let message = MessageType::load("std_msgs/msg/Float64").unwrap().message();
        // std_msgs/Float64 has exactly one double field at offset zero.
        unsafe {
            *message.data.cast::<f64>() = value;
        }
        assert_eq!(message.values().unwrap().0, json!({"data":null}));
        let message = MessageType::load("std_msgs/msg/Float32").unwrap().message();
        unsafe {
            *message.data.cast::<f32>() = value as f32;
        }
        assert_eq!(message.values().unwrap().0, json!({"data":null}));
    }
}

#[test]
fn time_now_and_ros1_aliases() {
    let typ = MessageType::load("builtin_interfaces/msg/Time").unwrap();
    for (input, expected) in [
        (json!("now"), json!({"sec":123,"nanosec":456})),
        (json!({"secs":3,"nsecs":5}), json!({"sec":3,"nanosec":5})),
    ] {
        let mut message = typ.message();
        message.fill(&input, (123, 456)).unwrap();
        assert_eq!(message.values().unwrap().0, expected);
    }
}
