export async function readStepRequest({ maximumBytes = Infinity } = {}) {
  if (maximumBytes !== Infinity && (!Number.isSafeInteger(maximumBytes) || maximumBytes <= 0)) {
    throw new RangeError("step request byte limit must be a positive integer");
  }
  let input = "";
  let bytes = 0;
  process.stdin.setEncoding("utf8");
  for await (const chunk of process.stdin) {
    bytes += Buffer.byteLength(chunk);
    if (bytes > maximumBytes) throw new RangeError("step request exceeds its byte limit");
    input += chunk;
  }
  return JSON.parse(input);
}
