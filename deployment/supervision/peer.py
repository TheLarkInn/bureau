"""Bounded Linux SO_PEERCRED bridge; no credentials or executable requests."""
import json
import os
import select
import socket
import struct
import sys
import time

MAXIMUM = 1024


def document(data):
    return json.loads(data.decode("utf-8"), parse_constant=lambda _: invalid())


def invalid():
    raise ValueError("invalid admission frame")


def read_socket(client):
    deadline = time.monotonic() + 2
    data = b""
    while not data.endswith(b"\n"):
        remaining = deadline - time.monotonic()
        if remaining <= 0 or len(data) > MAXIMUM:
            invalid()
        client.settimeout(remaining)
        part = client.recv(MAXIMUM + 1 - len(data))
        if not part:
            invalid()
        data += part
    if len(data) > MAXIMUM or data.count(b"\n") != 1:
        invalid()
    return document(data)


def read_reply():
    deadline = time.monotonic() + 2
    data = b""
    while not data.endswith(b"\n"):
        remaining = deadline - time.monotonic()
        if remaining <= 0 or len(data) >= MAXIMUM:
            invalid()
        if not select.select([sys.stdin.fileno()], [], [], remaining)[0]:
            invalid()
        part = os.read(sys.stdin.fileno(), 1)
        if not part:
            invalid()
        data += part
    return document(data)


def emit(value):
    data = (json.dumps(value, separators=(",", ":")) + "\n").encode("utf-8")
    if len(data) > 2048:
        invalid()
    sys.stdout.buffer.write(data)
    sys.stdout.buffer.flush()


def serve(path):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(path)
        os.chmod(path, 0o660)
        listener.listen(1)
        emit({"ready": True})
        sequence = 0
        while True:
            client, _ = listener.accept()
            with client:
                try:
                    request = read_socket(client)
                    peer = struct.unpack("3i", client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
                    sequence += 1
                    emit({"id": sequence, "peerPid": peer[0], "peerUid": peer[1], "peerGid": peer[2], "request": request})
                    reply = read_reply()
                    if set(reply) != {"id", "admitted"} or reply["id"] != sequence or reply["admitted"] is not True:
                        invalid()
                    client.sendall(b'{"admitted":true}\n')
                except (OSError, ValueError, TypeError):
                    continue


if __name__ == "__main__":
    if sys.platform != "linux" or len(sys.argv) != 2 or os.getuid() != 0:
        raise SystemExit("Linux root peer-credential bridge required")
    serve(sys.argv[1])
