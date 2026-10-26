// CDP eval helper: node cdp-eval.mjs <wsUrl> <expression>
// Connects to a CDP target, evaluates expression, prints JSON result.
const [, , wsUrl, expr] = process.argv;
if (!wsUrl || !expr) {
  console.error('usage: node cdp-eval.mjs <wsUrl> <expression>');
  process.exit(2);
}
const ws = new WebSocket(wsUrl);
let id = 0;
const pending = new Map();
function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const mid = ++id;
    pending.set(mid, { resolve, reject });
    ws.send(JSON.stringify({ id: mid, method, params }));
  });
}
ws.addEventListener('message', (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) reject(new Error(JSON.stringify(msg.error)));
    else resolve(msg.result);
  }
});
ws.addEventListener('open', async () => {
  try {
    const r = await send('Runtime.evaluate', {
      expression: expr,
      returnByValue: true,
      awaitPromise: true,
    });
    if (r.exceptionDetails) {
      console.log(JSON.stringify({ exception: r.exceptionDetails }));
    } else {
      console.log(JSON.stringify(r.result));
    }
  } catch (e) {
    console.log(JSON.stringify({ error: String(e) }));
  }
  ws.close();
  process.exit(0);
});
ws.addEventListener('error', (e) => {
  console.log(JSON.stringify({ wsError: String(e.message || e) }));
  process.exit(1);
});
setTimeout(() => {
  console.log(JSON.stringify({ timeout: true }));
  process.exit(1);
}, 8000);
