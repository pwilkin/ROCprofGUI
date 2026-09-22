import { expect, it } from "vitest";
import { sourceLocations } from "./sourceLocations";
it("parses each recorded inline location without swallowing the chain into a filename", () => {
  expect(sourceLocations("C:\\work\\inner.h:12 -> /src/outer.h:34")).toEqual([
    { reference: "C:\\work\\inner.h:12", path: "C:\\work\\inner.h", line: 12 },
    { reference: "/src/outer.h:34", path: "/src/outer.h", line: 34 },
  ]);
  expect(
    sourceLocations(
      "missing -> /src/a.h:0 -> /src/b.h:9007199254740992 -> /src/c.h:7",
    ),
  ).toEqual([{ reference: "/src/c.h:7", path: "/src/c.h", line: 7 }]);
  expect(sourceLocations("")).toEqual([]);
});
