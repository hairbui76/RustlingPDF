import { describe, expect, it } from "vitest";
import type { FileId } from "@app/types/file";
import { planLocalFilePathCarry } from "@app/hooks/tools/shared/localFilePathCarry";

const id = (value: string) => value as FileId;

/**
 * Every case here is "which file on the user's disk may this output
 * overwrite". Getting it wrong destroys a document, so the assertions are
 * mostly about *refusing* to carry a path.
 */
describe("planLocalFilePathCarry", () => {
  it("carries the path for a clean one-to-one operation", () => {
    const carry = planLocalFilePathCarry(
      [{ id: id("in1"), localFilePath: "/home/u/a.pdf" }],
      [{ id: id("out1"), sourceId: id("in1") }],
    );
    expect(carry).toEqual([
      { outputId: "out1", localFilePath: "/home/u/a.pdf" },
    ]);
  });

  it("carries each path independently for N inputs to N outputs", () => {
    const carry = planLocalFilePathCarry(
      [
        { id: id("in1"), localFilePath: "/home/u/a.pdf" },
        { id: id("in2"), localFilePath: "/home/u/b.pdf" },
      ],
      [
        { id: id("out1"), sourceId: id("in1") },
        { id: id("out2"), sourceId: id("in2") },
      ],
    );
    expect(carry).toEqual([
      { outputId: "out1", localFilePath: "/home/u/a.pdf" },
      { outputId: "out2", localFilePath: "/home/u/b.pdf" },
    ]);
  });

  it("follows recorded provenance, not output order", () => {
    // A backend that returns its ZIP members in a different order than the
    // uploads must not cause output 1 to overwrite input 1's file.
    const carry = planLocalFilePathCarry(
      [
        { id: id("in1"), localFilePath: "/home/u/a.pdf" },
        { id: id("in2"), localFilePath: "/home/u/b.pdf" },
      ],
      [
        { id: id("out1"), sourceId: id("in2") },
        { id: id("out2"), sourceId: id("in1") },
      ],
    );
    expect(carry).toEqual([
      { outputId: "out1", localFilePath: "/home/u/b.pdf" },
      { outputId: "out2", localFilePath: "/home/u/a.pdf" },
    ]);
  });

  it("carries nothing when one input fans out to several outputs", () => {
    // A split. Letting all three claim the source would run three overwrites
    // of the user's document and leave the last fragment in its place.
    expect(
      planLocalFilePathCarry(
        [{ id: id("in1"), localFilePath: "/home/u/book.pdf" }],
        [
          { id: id("out1"), sourceId: id("in1") },
          { id: id("out2"), sourceId: id("in1") },
          { id: id("out3"), sourceId: id("in1") },
        ],
      ),
    ).toEqual([]);
  });

  it("carries nothing when provenance is unknown", () => {
    // A single ZIP response: which member came from which upload is not
    // knowable, so guessing is not an option.
    expect(
      planLocalFilePathCarry(
        [
          { id: id("in1"), localFilePath: "/home/u/a.pdf" },
          { id: id("in2"), localFilePath: "/home/u/b.pdf" },
        ],
        [
          { id: id("out1"), sourceId: null },
          { id: id("out2"), sourceId: null },
        ],
      ),
    ).toEqual([]);
  });

  it("carries only the outputs whose provenance is known", () => {
    const carry = planLocalFilePathCarry(
      [
        { id: id("in1"), localFilePath: "/home/u/a.pdf" },
        { id: id("in2"), localFilePath: "/home/u/b.pdf" },
      ],
      [
        { id: id("out1"), sourceId: id("in1") },
        { id: id("out2"), sourceId: null },
      ],
    );
    expect(carry).toEqual([
      { outputId: "out1", localFilePath: "/home/u/a.pdf" },
    ]);
  });

  it("ignores inputs that were never on disk", () => {
    expect(
      planLocalFilePathCarry(
        [{ id: id("in1") }],
        [{ id: id("out1"), sourceId: id("in1") }],
      ),
    ).toEqual([]);
  });

  it("never lets two outputs claim the same path", () => {
    // Two stubs reporting one path should be impossible; if it ever happens,
    // the second claim is an overwrite of the first, so only one may win.
    const carry = planLocalFilePathCarry(
      [
        { id: id("in1"), localFilePath: "/home/u/same.pdf" },
        { id: id("in2"), localFilePath: "/home/u/same.pdf" },
      ],
      [
        { id: id("out1"), sourceId: id("in1") },
        { id: id("out2"), sourceId: id("in2") },
      ],
    );
    expect(carry).toHaveLength(1);
    expect(carry[0].localFilePath).toBe("/home/u/same.pdf");
  });
});
