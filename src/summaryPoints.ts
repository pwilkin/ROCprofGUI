export class PackedSummaryPoints implements Iterable<[number, number]> {
  private chunks: Float64Array[] = [];
  private count = 0;
  get length() { return this.count; }
  append(buffer: ArrayBuffer) {
    if (buffer.byteLength % 16 !== 0 || buffer.byteLength > 65536)
      throw Error('Invalid Activity sample chunk');
    if (!buffer.byteLength) return;
    this.chunks.push(new Float64Array(buffer));
    this.count += buffer.byteLength / 16;
  }
  *[Symbol.iterator](): Generator<[number, number]> {
    for (const values of this.chunks)
      for (let i = 0; i < values.length; i += 2)
        yield [values[i], values[i + 1]];
  }
}
