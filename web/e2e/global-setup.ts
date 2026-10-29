// The dev server compiles routes on first request, and a cold compile can serve a chunk
// with 500, which leaves the page stuck. Warm the routes before any test runs.
export default async function globalSetup(): Promise<void> {
  const base = 'http://localhost:3100';
  for (const path of ['/', '/history', '/payments/warmup']) {
    for (let attempt = 0; attempt < 3; attempt++) {
      const res = await fetch(`${base}${path}`);
      if (res.ok) break;
    }
  }
}
