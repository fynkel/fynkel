// Answers a request for part of a film: Safari plays no video without it,
// and Cloudflare's static files always come whole. Only /film/ comes here
// (wrangler.jsonc), and a film is a few megabytes.
export default {
  async fetch(request, env) {
    const whole = new Request(request);
    whole.headers.delete('range');
    const response = await env.ASSETS.fetch(whole);
    const range = /^bytes=(\d*)-(\d*)$/.exec(request.headers.get('range') ?? '');
    if (response.status !== 200 || request.method !== 'GET' || !range || range[0] === 'bytes=-') {
      return response;
    }
    const bytes = await response.arrayBuffer();
    const size = bytes.byteLength;
    // bytes=-n is the last n.
    const start = range[1] === '' ? Math.max(0, size - Number(range[2])) : Number(range[1]);
    const end = range[1] === '' || range[2] === '' ? size - 1 : Math.min(size - 1, Number(range[2]));
    const headers = new Headers(response.headers);
    headers.set('accept-ranges', 'bytes');
    if (start > end || start >= size) {
      headers.set('content-range', `bytes */${size}`);
      headers.delete('content-length');
      return new Response(null, { status: 416, headers });
    }
    headers.set('content-range', `bytes ${start}-${end}/${size}`);
    headers.set('content-length', String(end - start + 1));
    return new Response(bytes.slice(start, end + 1), { status: 206, headers });
  },
};
