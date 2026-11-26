// www.fynkel.com: sends every request to the same path on fynkel.com.
export default {
  fetch(request) {
    const url = new URL(request.url);
    url.hostname = 'fynkel.com';
    return Response.redirect(url.toString(), 301);
  },
};
