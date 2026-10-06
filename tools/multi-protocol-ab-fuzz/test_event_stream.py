import unittest

from abfuzz import _bodies_agree, event_frames
from seeds import event_frame, event_stream_seeds


class EventStreams(unittest.TestCase):
    def test_valid_seeds_and_corruption(self):
        for suite in ('multiprotocol', 'pokemon'):
            for protocol in ('rest-json1', 'rest-xml', 'aws-json-10', 'aws-json-11', 'rpcv2-cbor'):
                for label, _, _, _, body in event_stream_seeds(suite, protocol):
                    if label.endswith(('bad-crc', 'truncated')):
                        with self.assertRaises(ValueError):
                            event_frames(body)
                    else:
                        event_frames(body)

    def test_comparison_preserves_order_headers_and_binary_payloads(self):
        def frame(name, payload, content_type='application/json'):
            return event_frame({':message-type': 'event', ':event-type': name, ':content-type': content_type}, payload)
        a = frame('one', b'{"a":1,"b":2}')
        b = frame('one', b'{"b":2,"a":1}')
        other = frame('two', b'{}')
        self.assertTrue(_bodies_agree(a, b))
        self.assertFalse(_bodies_agree(a + other, other + b))
        self.assertFalse(_bodies_agree(a, a + other))
        self.assertFalse(_bodies_agree(a, a[:-1]))
        self.assertFalse(_bodies_agree(a, a[:-1] + bytes([a[-1] ^ 1])))
        self.assertFalse(_bodies_agree(frame('one', b'{"a":1,"b":2}', 'application/octet-stream'), frame('one', b'{"b":2,"a":1}', 'application/octet-stream')))


if __name__ == '__main__':
    unittest.main()
