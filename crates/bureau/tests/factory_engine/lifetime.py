"""A finite offline descendant. Socket EOF reports actual termination."""
from pathlib import Path
import socket
import sys
import time

marker, address = map(Path, sys.argv[1:])
channel = socket.socket(socket.AF_UNIX)
if address.exists():
    channel.connect("\0" + address.read_text())
time.sleep(10)
marker.write_text("alive")
channel.close()
