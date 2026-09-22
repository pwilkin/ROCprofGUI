import { it, expect } from "vitest";
import {
  globalMarks,
  dispatchMarks,
  defaultGlobalLayers,
} from "./globalTimeline";
import bundle from "../public/demo.json";
import type { Capture } from "./types";
it("global layers retain record identity, hardware scope, marker depth and missing ends", () => {
  const capture = {
    ...bundle.capture,
    end: 1000,
    intervals: [
      { start: 10, end: 90, se: 2, cu: 3, simd: 1, slot: 4, kernel: "same" },
    ],
    dispatches: [
      { ...bundle.capture.dispatches[0], id: "ignored", start: 0, end: 0 },
      {
        ...bundle.capture.dispatches[0],
        id: "42",
        kernel: "same",
        start: 10,
        end: 90,
      },
      {
        ...bundle.capture.dispatches[0],
        id: "43",
        kernel: "same",
        start: 100,
        end: 200,
      },
    ],
    markers: [
      {
        id: 0,
        markerId: 1,
        name: "open",
        kind: "scope",
        codeObject: "7",
        source: "a.hip:4",
        start: 30,
        end: null,
        point: false,
        depth: 2,
        se: 2,
        cu: 3,
        simd: 1,
        slot: 4,
        parent: null,
      },
      {
        id: 1,
        markerId: 2,
        name: "point",
        kind: "point",
        codeObject: "7",
        source: "",
        start: 35,
        end: null,
        point: true,
        depth: 3,
        se: 2,
        cu: 3,
        simd: 1,
        slot: 4,
        parent: 0,
      },
    ],
  } as unknown as Capture;
  const marks = globalMarks(capture, defaultGlobalLayers, "marker:0");
  expect(marks).toHaveLength(5);
  expect(marks[0]).toMatchObject({
    recordKind: "residency",
    recordIndex: 0,
    lane: "SE2 CU3 S1 slot4",
  });
  expect(marks[1]).toMatchObject({ start: 30, end: 1000, selected: true });
  expect(marks[1].label).toContain("unclosed scope");
  expect(marks[1].lane).toContain("depth 2");
  expect(marks[2]).toMatchObject({ start: 35, end: 35 });
  expect(marks[3]).toMatchObject({ key: "dispatch:1", recordIndex: 1 });
  expect(marks[3].label).toContain("Dispatch 42");
  expect(marks[4].label).toContain("Dispatch 43");
  expect(
    globalMarks(capture, {
      residency: false,
      dispatches: true,
      markers: false,
    }),
  ).toEqual(dispatchMarks(capture).map((m) => ({ ...m, selected: false })));
  expect(
    globalMarks(capture, {
      residency: false,
      dispatches: false,
      markers: false,
    }),
  ).toEqual([]);
  expect(capture.markers![0].end).toBeNull();
});

it("decoder events stay point records with exact payload text and separate identities", () => {
  const capture = {
    ...bundle.capture,
    intervals: [],
    dispatches: [],
    markers: [],
    decoderEvents: [
      {
        id: "2:0",
        se: 2,
        time: 5,
        kind: "event",
        fields: {
          type: "6",
          payload: "18446744073709551615",
          me_id: "1",
          pipe_id: "0",
        },
      },
      {
        id: "2:1",
        se: 2,
        time: 10,
        kind: "dispatch",
        fields: { kernel_name: "same" },
      },
      { id: "2:2", se: 2, time: 11, kind: "event", fields: { type: "999" } },
    ],
  } as unknown as Capture;
  const marks = globalMarks(capture, defaultGlobalLayers);
  expect(marks.map((m) => [m.start, m.end])).toEqual([
    [5, 5],
    [10, 10],
    [11, 11],
  ]);
  expect(marks[0].label).toContain("Packet loss");
  expect(marks[0].label).toContain("18446744073709551615");
  expect(marks[1].label).toContain("Dispatch start");
  expect(marks[2].label).toContain("Decoder event type 999");
  expect(
    globalMarks(capture, { ...defaultGlobalLayers, events: false }),
  ).toEqual([]);
});

it("event groups match upstream and retain unknown records in Other", async () => {
  const { decoderEventGroup, defaultEventGroups } =
    await import("./globalTimeline");
  const events = Array.from({ length: 18 }, (_, type) => ({
    id: String(type),
    se: 0,
    time: type,
    kind: "event",
    fields: { type: String(type) },
  }));
  expect(events.map(decoderEventGroup)).toEqual([
    32, 2, 2, 32, 32, 2, 32, 4, 4, 8, 8, 8, 32, 32, 32, 32, 16, 32,
  ]);
  expect(decoderEventGroup({ ...events[0], kind: "future" })).toBe(32);
  expect(decoderEventGroup({ ...events[0], kind: "dispatch" })).toBe(1);
  const capture = {
    ...bundle.capture,
    intervals: [],
    dispatches: [],
    markers: [],
    decoderEvents: events,
  } as unknown as Capture;
  expect(defaultEventGroups).toBe(47);
  expect(globalMarks(capture, defaultGlobalLayers)).toHaveLength(17);
  expect(
    globalMarks(capture, { ...defaultGlobalLayers, eventGroups: 0 }),
  ).toEqual([]);
  expect(
    globalMarks(capture, { ...defaultGlobalLayers, eventGroups: 16 }).map(
      (m) => m.recordIndex,
    ),
  ).toEqual([16]);
  expect(
    globalMarks(capture, { ...defaultGlobalLayers, eventGroups: 63 }),
  ).toHaveLength(18);
});

it("CU events follow upstream exclusions and the selected shader engine", async () => {
  const { decoderMarks } = await import("./globalTimeline");
  const events = [1, 7, 8, 17, 6, 16, 999].map((type, index) => ({
    id: String(index),
    se: 2,
    time: index,
    kind: "event",
    fields: { type: String(type) },
  }));
  const capture = {
    ...bundle.capture,
    decoderEvents: [
      ...events,
      { ...events[4], id: "otherSE", se: 3 },
      { ...events[0], id: "dispatch", kind: "dispatch" },
    ],
  } as unknown as Capture;
  const layers = { ...defaultGlobalLayers, eventGroups: 63 };
  expect(
    decoderMarks(capture, layers, "cu", 2).map((m) => m.recordIndex),
  ).toEqual([4, 5, 6, 8]);
  expect(decoderMarks(capture, layers, "global")).toHaveLength(9);
  expect(decoderMarks(capture, { ...layers, events: false }, "cu", 2)).toEqual(
    [],
  );
});
