"""Independent functional G.711 reference hashes; CPython 3.12 only.

No upstream source/tables or recorded audio are copied. Inputs are all signed
16-bit samples in ascending order, and all octets in ascending order. SHA-256
covers encoded octets and decoded little-endian i16 bytes respectively.
"""
import hashlib
import json
import struct
import sys
import warnings

if sys.version_info[:2] != (3, 12) or sys.byteorder != "little":
    raise SystemExit("Use pinned CPython 3.12 on little-endian Linux for these reference vectors.")
with warnings.catch_warnings():
    warnings.simplefilter("ignore", DeprecationWarning)
    import audioop

pcm = b"".join(struct.pack("<h", sample) for sample in range(-32768, 32768))
result = {"schema_version": 1, "oracle": "CPython " + sys.version.split()[0] + " audioop",
          "input_order": "signed i16 ascending; octets ascending; decoded PCM little-endian",
          "vectors": {}}
for codec, encode, decode in (("PCMA", audioop.lin2alaw, audioop.alaw2lin),
                             ("PCMU", audioop.lin2ulaw, audioop.ulaw2lin)):
    result["vectors"][codec] = {
        "all_65536_encodings_sha256": hashlib.sha256(encode(pcm, 2)).hexdigest(),
        "all_256_decodings_sha256": hashlib.sha256(decode(bytes(range(256)), 2)).hexdigest(),
    }
print(json.dumps(result, indent=2))
