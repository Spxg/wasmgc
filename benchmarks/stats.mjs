// Two-tailed 95% Student's t critical values, as used by Benchmark.js.
const criticalValues = [
  12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228,
  2.201, 2.179, 2.16, 2.145, 2.131, 2.12, 2.11, 2.101, 2.093, 2.086,
  2.08, 2.074, 2.069, 2.064, 2.06, 2.056, 2.052, 2.048, 2.045, 2.042,
];

export function summarize(stats, operations = 1) {
  const { avg, samples } = stats;
  if (!Number.isFinite(avg) || avg <= 0 || !Array.isArray(samples) || samples.length < 2) {
    throw new Error("Expected a positive mean and at least two timing samples");
  }
  if (!Number.isInteger(operations) || operations <= 0) {
    throw new Error("Expected a positive operation count");
  }

  // Mitata samples are nanoseconds per callback, after its batch normalization
  // and trimming. The margin estimates the retained samples' mean at 95% confidence.
  // A sample can be zero when both readings fall within the same clock tick.
  let sumSquares = 0;
  for (const sample of samples) {
    if (!Number.isFinite(sample) || sample < 0) {
      throw new Error("Expected non-negative finite timing samples");
    }
    sumSquares += (sample - avg) ** 2;
  }

  const sampleCount = samples.length;
  const variance = sumSquares / (sampleCount - 1);
  const critical = criticalValues[sampleCount - 2] ?? 1.96;
  const margin = critical * Math.sqrt(variance / sampleCount);
  const rme = margin / avg * 100;
  if (!Number.isFinite(rme)) {
    throw new Error("Timing samples produced invalid statistics");
  }

  return { rme, sampleCount, meanNs: avg / operations, marginNs: margin / operations };
}
