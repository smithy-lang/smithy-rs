#!/usr/bin/env python
import sys
import json


def main():
    with open(sys.argv[1]) as f:
        model = json.load(f)

    service = [v for k, v in model['shapes'].items() if v['type'] == 'service'][0]
    sdk_id = service['traits']['aws.api#service']['sdkId']
    print(json.dumps(dict(sdk_id=sdk_id, crate_name=sdk_id.lower().replace(' ', ''))))


if __name__ == "__main__":
    main()
