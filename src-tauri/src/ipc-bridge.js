// Shift Browser IPC 桥
// WebKitGTK 的自定义协议页面（shift://）不执行 UserScript，导致 Tauri 的
// window.ipc / window.__TAURI_INTERNALS__ 无法注入。这里手动注入最小必要集，
// 优先走 ipc:// 自定义协议 fetch（tauri 原生方式，不依赖 web extension），
// 失败时回退到 window.webkit.messageHandlers['ipc']（wry 已注册）。
(function () {
  function showDiag(msg) {
    try {
      var d = document.createElement('div');
      d.style.cssText = 'position:fixed;bottom:10px;right:10px;z-index:99999;background:#c00;color:#fff;padding:8px 12px;font-size:12px;border-radius:6px;max-width:70%;';
      d.textContent = '[ipc-bridge] ' + msg;
      document.body.appendChild(d);
    } catch (_) {}
  }

  if (window.__TAURI_INTERNALS__) {
    showDiag('TAURI_INTERNALS 已存在（UserScript 执行了）');
    return;
  }

  var callbacks = new Map();

  function transformCallback(callback, once) {
    var id = window.crypto.getRandomValues(new Uint32Array(1))[0];
    callbacks.set(id, function (event) {
      if (once) callbacks.delete(id);
      if (callback) callback(event);
    });
    return id;
  }

  function unregisterCallback(id) {
    callbacks.delete(id);
  }

  function runCallback(id, arg) {
    var cb = callbacks.get(id);
    if (cb) {
      callbacks.delete(id);
      cb(arg);
    }
  }

  var customProtocolIpcFailed = false;

  function processIpcMessage(payload) {
    // 简化：我们的 payload 都是可 JSON 序列化的普通对象
    return { contentType: 'application/json', data: JSON.stringify(payload) };
  }

  function sendIpcMessage(message) {
    var cmd = message.cmd;
    var callback = message.callback;
    var error = message.error;
    var payload = message.payload;
    var options = message.options;

    // 优先：ipc:// 自定义协议 fetch（tauri 默认方式）
    if (!customProtocolIpcFailed) {
      var processed = processIpcMessage(payload);
      var headers = new Headers((options && options.headers) || {});
      headers.set('Content-Type', processed.contentType);
      headers.set('Tauri-Callback', String(callback));
      headers.set('Tauri-Error', String(error));
      headers.set('Tauri-Invoke-Key', '__INVOKE_KEY_PLACEHOLDER__');

      fetch('ipc://localhost/' + encodeURIComponent(cmd), {
        method: 'POST',
        body: processed.data,
        headers: headers
      })
        .then(function (response) {
          showDiag('fetch 响应 status=' + response.status);
          var callbackId =
            response.headers.get('Tauri-Response') === 'ok' ? callback : error;
          var ct = (response.headers.get('content-type') || '').split(',')[0];
          if (ct === 'application/json') {
            return response.json().then(function (r) { return [callbackId, r]; });
          } else if (ct === 'text/plain') {
            return response.text().then(function (r) { return [callbackId, r]; });
          } else {
            return response.arrayBuffer().then(function (r) { return [callbackId, r]; });
          }
        })
        .then(function (result) {
          window.__TAURI_INTERNALS__.runCallback(result[0], result[1]);
        })
        .catch(function (e) {
          // 自定义协议失败，回退到 postMessage 接口
          console.warn('[ipc-bridge] ipc:// fetch 失败，回退 postMessage', e);
          showDiag('ipc:// fetch 失败: ' + e);
          customProtocolIpcFailed = true;
          sendIpcMessage(message);
        });
    } else {
      // 回退：window.webkit.messageHandlers['ipc']（wry attach_ipc_handler 注册）
      var data = JSON.stringify({
        cmd: cmd,
        callback: callback,
        error: error,
        options: Object.assign({}, options, { customProtocolIpcBlocked: true }),
        payload: payload,
        __TAURI_INVOKE_KEY__: '__INVOKE_KEY_PLACEHOLDER__'
      });
      if (
        window.ipc &&
        window.webkit &&
        window.webkit.messageHandlers &&
        window.webkit.messageHandlers['ipc']
      ) {
        window.ipc.postMessage(data);
      } else {
        console.error('[ipc-bridge] 无可用 IPC 通道');
        showDiag('无 IPC 通道（messageHandlers 不存在）');
      }
    }
  }

  window.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { windowLabel: 'main', label: 'main' }
    },
    plugins: { path: { delimiter: ':', sep: '/' } },
    transformCallback: transformCallback,
    unregisterCallback: unregisterCallback,
    runCallback: runCallback,
    callbacks: callbacks,
    invoke: function (cmd, args, options) {
      return new Promise(function (resolve, reject) {
        var callback = transformCallback(function (result) { resolve(result); }, true);
        var error = transformCallback(function (err) { reject(err); }, true);
        sendIpcMessage({ cmd: cmd, callback: callback, error: error, payload: args, options: options });
      });
    },
    postMessage: sendIpcMessage,
    convertFileSrc: function (path, protocol) {
      return protocol + '://localhost/' + encodeURIComponent(path);
    },
    __TAURI_PATTERN__: { pattern: 'brownfield' }
  };
})();