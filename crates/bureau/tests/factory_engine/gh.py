"""Offline gh environment probe; never executes gh or makes a forge request."""
import json
import os

print(json.dumps(dict(os.environ)))
