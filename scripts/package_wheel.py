#
# Copyright (c) 2026 Wing Mun Fung
#
# This program and the accompanying materials are made available under the
# terms of the Eclipse Public License 2.0, available at
# https://www.eclipse.org/legal/epl-2.0/, or the Apache License, Version 2.0,
# available at https://www.apache.org/licenses/LICENSE-2.0.
#
# SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
#

"""Build a platform wheel containing tested Humble and Jazzy binaries."""

import base64
import hashlib
import sys
import zipfile
from pathlib import Path


def digest(data):
    value = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"sha256={value}"


def main():
    version, architecture, humble_binary, jazzy_binary, output = sys.argv[1:]
    platform = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64"}[architecture]
    distribution = "rosbridge_server_rs"
    wheel_name = f"{distribution}-{version}-py3-none-manylinux_2_35_{platform}.whl"
    package = f"{distribution}/"
    metadata = f"{distribution}-{version}.dist-info/"
    files = {
        package + "__init__.py": b"",
        metadata + "licenses/LICENSE": Path("LICENSE").read_bytes(),
        package
        + "__main__.py": (
            b"import os\nimport sys\nfrom pathlib import Path\n\n"
            b"SUPPORTED = ('humble', 'jazzy')\n\n"
            b"def main():\n"
            b"    distro = os.environ.get('ROS_DISTRO')\n"
            b"    if distro not in SUPPORTED:\n"
            b"        choices = ', '.join(SUPPORTED)\n"
            b"        raise SystemExit(f'Source ROS 2 first; supported ROS_DISTRO values: {choices}')\n"
            b"    binary = Path(__file__).with_name(f'rosbridge_server_rs_{distro}')\n"
            b"    os.execv(binary, [str(binary), *sys.argv[1:]])\n"
        ),
        metadata
        + "METADATA": (
            "Metadata-Version: 2.4\n"
            "Name: rosbridge-server-rs\n"
            f"Version: {version}\n"
            "Summary: ROS 2 rosbridge WebSocket server implemented in Rust\n"
            "License-Expression: EPL-2.0 OR Apache-2.0\n"
            "License-File: LICENSE\n"
            "Requires-Python: >=3.8\n"
            "Project-URL: Repository, https://github.com/yumeminami/rosbridge_server_rs\n"
        ).encode(),
        metadata
        + "WHEEL": (
            "Wheel-Version: 1.0\n"
            "Generator: rosbridge_server_rs scripts/package_wheel.py\n"
            "Root-Is-Purelib: false\n"
            f"Tag: py3-none-manylinux_2_35_{platform}\n"
        ).encode(),
        metadata
        + "entry_points.txt": (
            "[console_scripts]\n"
            "rosbridge_server_rs = rosbridge_server_rs.__main__:main\n"
            "rosbridge-server-rs = rosbridge_server_rs.__main__:main\n"
        ).encode(),
    }
    files[package + "rosbridge_server_rs_humble"] = Path(humble_binary).read_bytes()
    files[package + "rosbridge_server_rs_jazzy"] = Path(jazzy_binary).read_bytes()
    rows = [(name, digest(data), str(len(data))) for name, data in files.items()]
    record = metadata + "RECORD"
    rows.append((record, "", ""))
    record_data = "".join(",".join(row) + "\n" for row in rows).encode()
    destination = Path(output) / wheel_name
    with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, data in files.items():
            info = zipfile.ZipInfo(name)
            info.compress_type = zipfile.ZIP_DEFLATED
            executable = "/rosbridge_server_rs_" in name
            info.external_attr = (0o755 if executable else 0o644) << 16
            archive.writestr(info, data)
        archive.writestr(record, record_data)
    print(destination)


if __name__ == "__main__":
    main()
