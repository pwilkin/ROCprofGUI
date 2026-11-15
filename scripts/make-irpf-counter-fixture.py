#!/usr/bin/env python3
"""Writes a small synthetic IRPF bundle with the hardware-counter families.

Layouts follow hrx-system runtime/src/iree/hal/profile_file.h and
profile_schema.h (counter-sets 32 bytes + name, counters 48 + strings,
counter-samples 112 + 8 per value, counter-captures 88). The bundle is NOT a
hardware capture: every number in it is made up so the importer and the
collection report can be tested without a GPU.

    make_counter_fixture.py OUT.irpf
"""
import struct
import sys

PREFIX = "application/vnd.iree.hal.profile."
NONE = 0xFFFFFFFF


def record(rtype, family, producer, session, payload=b"", stream=0, device=NONE, queue=NONE, flags=0, dropped=0):
    content_type = (PREFIX + family).encode()
    name = producer.encode()
    total = 104 + len(content_type) + len(name) + len(payload)
    header = bytearray(104)
    struct.pack_into("<QQQQ", header, 0, total, len(payload), session, stream)
    struct.pack_into("<IIIII", header, 56, 104, len(content_type), len(name), device, queue)
    struct.pack_into("<QQI", header, 80, flags, dropped, 0)
    struct.pack_into("<H", header, 100, rtype)
    return bytes(header) + content_type + name + payload


def device(ordinal, frequency):
    r = bytearray(40)
    struct.pack_into("<IIII", r, 0, 40, 1 << 1, ordinal, 1)
    struct.pack_into("<Q", r, 32, frequency)
    return bytes(r)


def queue(dev, ordinal, stream):
    r = bytearray(24)
    struct.pack_into("<III", r, 0, 24, dev, ordinal)
    struct.pack_into("<Q", r, 16, stream)
    return bytes(r)


def function(exe, ordinal, name):
    r = bytearray(64)
    struct.pack_into("<I", r, 0, 64 + len(name))
    struct.pack_into("<QI", r, 8, exe, ordinal)
    struct.pack_into("<I", r, 44, len(name))
    return bytes(r) + name.encode()


def dispatch_event(event_id, exe, ordinal, count, size, start, end):
    r = bytearray(88)
    struct.pack_into("<I", r, 0, 88)
    struct.pack_into("<Q", r, 8, event_id)
    struct.pack_into("<Q", r, 16, 1)  # submission
    struct.pack_into("<Q", r, 32, exe)
    struct.pack_into("<I", r, 44, ordinal)
    struct.pack_into("<III", r, 48, *count)
    struct.pack_into("<III", r, 60, *size)
    struct.pack_into("<QQ", r, 72, start, end)
    return bytes(r)


def counter_set(set_id, dev, counters, values, name):
    r = struct.pack("<IIQIIII", 32 + len(name), 0, set_id, dev, counters, values, len(name))
    return r + name.encode()


def counter(set_id, ordinal, unit, offset, count, block, name, description):
    total = 48 + len(block) + len(name) + len(description)
    r = struct.pack("<IIIIQIIIIII", total, 1, unit, 0, set_id, ordinal, offset, count, len(block), len(name), len(description))
    return r + block.encode() + name.encode() + description.encode()


def counter_sample(sample_id, set_id, flags, event_id, command_index, start, end, values):
    r = bytearray(112)
    struct.pack_into("<IIII", r, 0, 112 + 8 * len(values), flags, 1, 0)
    struct.pack_into("<QQQQQQQQQ", r, 16, sample_id, set_id, event_id, 1, 7, 1, 1, start, end)
    struct.pack_into("<IIIIII", r, 88, command_index, 0, 0, 0, len(values), 0)
    return bytes(r) + b"".join(struct.pack("<Q", v) for v in values)


def counter_capture(set_id, flags, engines, instances, limit, operations, dispatches, skipped, dropped, invalid, foreign):
    return struct.pack("<IIQIIIIQQQQQQQ", 88, flags, set_id, 0, engines, instances, 0, limit, operations, dispatches, skipped, dropped, invalid, foreign)


def main(out):
    gpu = lambda family, payload, **kw: record(2, family, "amd-gpu", 1, payload, device=0, **kw)
    # Three dispatches of one kernel: 4, 8 and 2 workgroups of 64 x 1 x 1.
    events = b"".join(
        dispatch_event(i + 1, 1, 0, (groups, 1, 1), (64, 1, 1), start, end)
        for i, (groups, start, end) in enumerate([(4, 1000, 1200), (8, 1400, 1800), (2, 2000, 2100)])
    )
    names = [
        ("SQG_WAVES", 1, "Waves launched"),
        ("SQG_WAVES_32", 1, "Wave32 waves launched"),
        ("SQG_WAVES_64", 1, "Wave64 waves launched"),
        ("SQG_ITEMS", 1, "Work items launched"),
        ("SQG_BUSY_CYCLES", 2, "Cycles with a wave in the shader engine"),
    ]
    counters = b"".join(counter(1, i, unit, 2 * i, 2, "SQG", name, text) for i, (name, unit, text) in enumerate(names))
    # Per sample: one value per shader engine for each counter. Two dispatches
    # are sampled (the bound is 2), so the third has no counter row.
    samples = b"".join(
        counter_sample(i + 1, 1, 0b111, i + 1, i, start, end, [g // 2, g - g // 2, 0, 0, g // 2, g - g // 2, 32 * g, 32 * g, cycles, cycles + 9])
        for i, (g, start, end, cycles) in enumerate([(4, 1000, 1200, 4100), (8, 1400, 1800, 9050)])
    )
    capture = counter_capture(1, 0b1111, 2, 0, 2, 1, 2, 0, 0, 0, 0)
    body = b"".join(
        [
            record(1, "session", "amd-gpu", 1),
            gpu("devices", device(0, 100_000_000)),
            gpu("queues", queue(0, 0, 1), queue=0, stream=1),
            gpu("executable-functions", function(1, 0, "g_gemm_q4_k_store_m256")),
            gpu("dispatch-events", events, queue=0, stream=1),
            gpu("counter-sets", counter_set(1, 0, len(names), 2 * len(names), "amdgpu.perfmon")),
            gpu("counters", counters),
            gpu("counter-samples", samples, queue=0, stream=1),
            gpu("counter-captures", capture),
            record(3, "session", "amd-gpu", 1),
        ]
    )
    header = struct.pack("<IHHIIQ", 0x46505249, 2, 0, 24, 0, 24 + len(body))
    with open(out, "wb") as f:
        f.write(header + body)
    print(f"{out}: {24 + len(body)} bytes")


if __name__ == "__main__":
    main(sys.argv[1])
