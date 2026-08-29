// ==UserScript==
// @name         清华教参一键下载（thubookrs）
// @namespace    https://github.com/Ricky1911/thubookrs
// @version      1.1.0
// @description  调用本机 thubookrs Rust 服务下载教参并生成 PDF
// @match        https://ereserves.lib.tsinghua.edu.cn/*
// @run-at       document-start
// @grant        GM.xmlHttpRequest
// @grant        GM.getValue
// @grant        GM.setValue
// @connect      127.0.0.1
// ==/UserScript==

(async () => {
  'use strict';

  const API = 'http://127.0.0.1:19110/v1';
  const TOKEN_KEY = 'thubookrs-platform-token';
  const SECRET_KEY = 'thubookrs-local-secret';

  function tokenFromUrl(value) {
    try {
      const url = new URL(value, location.href);
      const token = url.searchParams.get('token');
      return token && token.length > 10 ? token : null;
    } catch (_) {
      return null;
    }
  }

  async function rememberToken() {
    let token = tokenFromUrl(location.href);
    if (!token) {
      for (const entry of performance.getEntriesByType('resource')) {
        token = tokenFromUrl(entry.name);
        if (token) break;
      }
    }
    if (token) await GM.setValue(TOKEN_KEY, token);
  }

  function request(method, path, body, headers = {}) {
    return new Promise((resolve, reject) => {
      GM.xmlHttpRequest({
        method,
        url: `${API}${path}`,
        headers: { 'Content-Type': 'application/json', ...headers },
        data: body === undefined ? undefined : JSON.stringify(body),
        timeout: 10000,
        onload(response) {
          let data;
          try { data = JSON.parse(response.responseText || '{}'); }
          catch (_) { data = { error: response.responseText }; }
          if (response.status >= 200 && response.status < 300) resolve(data);
          else reject(new Error(data.error || `本地服务返回 ${response.status}`));
        },
        ontimeout: () => reject(new Error('连接本地服务超时')),
        onerror: () => reject(new Error('未检测到 thubookrs，请先双击运行 thubookrs.exe')),
      });
    });
  }

  async function getSecret() {
    let secret = await GM.getValue(SECRET_KEY, '');
    if (secret) return secret;
    const result = await request('POST', '/pair', undefined, { 'X-Thubookrs-Pair': 'userscript-v1' });
    secret = result.secret;
    await GM.setValue(SECRET_KEY, secret);
    return secret;
  }

  async function authorizedRequest(method, path, body) {
    let secret = await getSecret();
    try {
      return await request(method, path, body, { Authorization: `Bearer ${secret}` });
    } catch (error) {
      if (!/未配对/.test(error.message)) throw error;
      await GM.setValue(SECRET_KEY, '');
      secret = await getSecret();
      return request(method, path, body, { Authorization: `Bearer ${secret}` });
    }
  }

  await rememberToken();
  if (!location.pathname.startsWith('/bookDetail/')) return;

  const ready = () => new Promise(resolve => {
    if (document.body) resolve();
    else addEventListener('DOMContentLoaded', resolve, { once: true });
  });
  await ready();

  const style = document.createElement('style');
  style.textContent = `
    #thubookrs-panel{position:fixed;right:24px;bottom:24px;z-index:2147483647;width:300px;padding:18px;background:#fff;color:#17202a;border:1px solid #d9e2ec;border-radius:14px;box-shadow:0 12px 38px rgba(15,35,55,.2);font:14px/1.45 system-ui,"Microsoft YaHei",sans-serif}
    #thubookrs-panel h3{margin:0 0 12px;font-size:17px}#thubookrs-panel label{display:flex;align-items:center;justify-content:space-between;margin:8px 0}
    #thubookrs-panel select{width:110px;padding:5px;border:1px solid #bcccdc;border-radius:6px;background:#fff}
    #thubookrs-panel button{width:100%;margin-top:10px;padding:9px;border:0;border-radius:8px;background:#6f2c91;color:#fff;font-weight:600;cursor:pointer}
    #thubookrs-panel button[disabled]{opacity:.55;cursor:not-allowed}.thubookrs-progress{height:8px;margin-top:12px;background:#e8edf2;border-radius:8px;overflow:hidden}
    .thubookrs-progress i{display:block;width:0;height:100%;background:#6f2c91;transition:width .25s}.thubookrs-status{margin-top:8px;color:#52606d;word-break:break-all}
  `;
  document.head.appendChild(style);

  const panel = document.createElement('section');
  panel.id = 'thubookrs-panel';
  panel.innerHTML = `
    <h3>下载为 PDF</h3>
    <label>下载线程<select id="thubookrs-threads"><option>2</option><option selected>4</option><option>8</option><option>16</option></select></label>
    <label>清晰度<select id="thubookrs-quality"><option value="6">标准</option><option value="8">高清</option><option value="10" selected>最高</option></select></label>
    <label><span>统一页面尺寸</span><input id="thubookrs-resize" type="checkbox"></label>
    <label><span>删除临时图片</span><input id="thubookrs-delete" type="checkbox" checked></label>
    <button id="thubookrs-start">开始下载</button>
    <div class="thubookrs-progress"><i></i></div>
    <div class="thubookrs-status">请先确保 thubookrs.exe 正在运行</div>
  `;
  document.body.appendChild(panel);

  const button = panel.querySelector('#thubookrs-start');
  const status = panel.querySelector('.thubookrs-status');
  const bar = panel.querySelector('.thubookrs-progress i');

  button.addEventListener('click', async () => {
    button.disabled = true;
    status.textContent = '正在连接本地 Rust 服务…';
    try {
      await rememberToken();
      const token = await GM.getValue(TOKEN_KEY, '');
      if (!token) throw new Error('没有获取到登录 token，请退出平台后重新登录一次');
      const task = await authorizedRequest('POST', '/jobs', {
        url: location.href.split('?')[0],
        token,
        threads: Number(panel.querySelector('#thubookrs-threads').value),
        quality: Number(panel.querySelector('#thubookrs-quality').value),
        auto_resize: panel.querySelector('#thubookrs-resize').checked,
        delete_images: panel.querySelector('#thubookrs-delete').checked,
      });
      status.textContent = '任务已经提交，正在读取书籍信息…';
      const timer = setInterval(async () => {
        try {
          const job = await authorizedRequest('GET', `/jobs/${task.id}`);
          bar.style.width = `${job.percent || 0}%`;
          status.textContent = job.status === 'converting'
            ? '页面下载完成，正在生成 PDF…'
            : job.total
              ? `正在下载 ${job.current} / ${job.total} 页（${job.percent.toFixed(1)}%）`
              : '正在读取书籍信息…';
          if (job.status === 'completed') {
            clearInterval(timer);
            bar.style.width = '100%';
            status.textContent = `下载完成：${job.output}`;
            button.disabled = false;
          } else if (job.status === 'failed' || job.status === 'cancelled') {
            clearInterval(timer);
            status.textContent = job.error || '任务已取消';
            button.disabled = false;
          }
        } catch (error) {
          clearInterval(timer);
          status.textContent = error.message;
          button.disabled = false;
        }
      }, 1000);
    } catch (error) {
      status.textContent = error.message;
      button.disabled = false;
    }
  });
})();
