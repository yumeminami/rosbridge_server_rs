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

"""Run an unmodified upstream WebSocket test with a supplied server port."""
import importlib.util
import sys
import unittest
from pathlib import Path

path, port = Path(sys.argv[1]), int(sys.argv[2])
sys.path.insert(0, str(path.parent))
import common


async def supplied_port(node):
    return port


common.get_server_port = supplied_port
spec = importlib.util.spec_from_file_location("upstream_case", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
result = unittest.TextTestRunner(verbosity=2).run(
    unittest.defaultTestLoader.loadTestsFromModule(module)
)
sys.exit(not result.wasSuccessful())
