#!/usr/bin/env python3
import json
import base64
import struct

with open('aws/sdk/aws-models/s3.json') as f:
    data = json.load(f)

nodes_b64 = data['shapes']['com.amazonaws.s3#AmazonS3']['traits']['smithy.rules#endpointBdd']['nodes']
nodes_bytes = base64.b64decode(nodes_b64)

nodes = []
for i in range(0, len(nodes_bytes), 12):
    conditionIndex, highRef, lowRef = struct.unpack('>iii', nodes_bytes[i:i+12])
    nodes.append([conditionIndex, highRef, lowRef])

print(json.dumps(nodes, indent=2))
