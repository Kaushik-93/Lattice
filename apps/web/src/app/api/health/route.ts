// Container health probe. The only kind of route handler the web app has: data never
// flows through Next.js (spec §2.1).
export function GET() {
  return Response.json({ status: "ok" });
}
