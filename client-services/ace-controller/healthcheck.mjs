const base = process.env.VITE_ACE_HTTP_URL || "http://127.0.0.1:8100";

try {
  const res = await fetch(`${base}/health`);
  const json = await res.json();
  console.log(JSON.stringify(json, null, 2));
  process.exit(res.ok ? 0 : 1);
} catch (err) {
  console.error("ace-controller unreachable:", err.message);
  process.exit(1);
}
