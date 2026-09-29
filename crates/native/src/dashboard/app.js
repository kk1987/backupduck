"use strict";
const $ = id => document.getElementById(id);
const en = !navigator.language.toLowerCase().startsWith("zh");
const words = en ? {
  title:"Receiver management", local:"Local network", loginTitle:"Enter the code shown on your phone", loginHelp:"Find it in PhotoBridge on your Pixel: Settings → Browser management. It changes each time the page is enabled.", code:"Access code", enter:"Open dashboard", received:"Received", published:"Added to phone gallery", failed:"Needs attention", storage:"Originals kept", transfers:"Transfers", cloud:"Added to the phone gallery does not mean Google Photos has backed it up.", all:"All", processing:"Processing", done:"In gallery", failedFilter:"Failed", refresh:"Refresh", retry:"Retry failures", more:"Load more", empty:"No matching transfers", loginError:"Check the code on your phone and try again.", locked:"Too many attempts. Try again in five minutes.", connectionError:"Could not reach the Pixel. Check that receiving and browser management are still on.", retried:"Items queued for retry: ", receivedState:"Received · Waiting for gallery", receivingState:"Receiving", failedState:"Received · Gallery processing failed", doneState:"In phone gallery", conversion:"Conversion needed · Enable it on the Pixel before retrying"
} : {
  title:"接收端管理", local:"局域网", loginTitle:"输入手机上的访问码", loginHelp:"在 Pixel 的 PhotoBridge「设置 → 浏览器管理」中查看。每次开启都会更换访问码。", code:"访问码", enter:"进入管理页", received:"已接收", published:"已加入手机相册", failed:"待处理失败", storage:"保留的原件", transfers:"传输记录", cloud:"加入手机相册不代表 Google 相册云端已备份。", all:"全部", processing:"处理中", done:"已加入相册", failedFilter:"处理失败", refresh:"刷新", retry:"重试失败项", more:"加载更多", empty:"没有符合条件的记录", loginError:"访问码不对，请查看手机后重试。", locked:"尝试次数过多，请五分钟后重试。", connectionError:"无法连接 Pixel，请确认接收和浏览器管理仍已开启。", retried:"已重新排队：", receivedState:"已接收 · 等待加入相册", receivingState:"接收中", failedState:"已接收 · 加入相册失败", doneState:"已加入手机相册", conversion:"需要兼容转换 · 在 Pixel 开启后再重试"
};
for (const [id,key] of Object.entries({"page-title":"title","connection":"local","login-title":"loginTitle","login-help":"loginHelp","code-label":"code","login-button":"enter","label-received":"received","label-published":"published","label-failed":"failed","label-storage":"storage","transfers-title":"transfers","cloud-note":"cloud","refresh":"refresh","retry":"retry","more":"more"})) $(id).textContent = words[key];
for (const [value,key] of Object.entries({all:"all",processing:"processing",published:"done",failed:"failedFilter"})) $("state-filter").querySelector(`option[value="${value}"]`).textContent = words[key];
let token = sessionStorage.getItem("photobridge-dashboard-token") || "";
let cursor = null;
let loading = false;
let paged = false;
const message = text => { $("message").textContent = text; };
const size = bytes => { const unit = bytes >= 1048576 ? "MB" : "KB"; return `${(bytes / (unit === "MB" ? 1048576 : 1024)).toFixed(1)} ${unit}`; };
async function api(path, options = {}) {
  const headers = { ...(options.headers || {}) };
  if (token) headers.Authorization = `Bearer ${token}`;
  const response = await fetch(path, { ...options, headers, cache:"no-store" });
  if (response.status === 401 && path !== "/api/login") { token = ""; sessionStorage.removeItem("photobridge-dashboard-token"); $("login-panel").hidden = false; $("dashboard").hidden = true; throw new Error("login"); }
  if (!response.ok) throw new Error(String(response.status));
  return response.json();
}
function stateLabel(item) {
  if (item.processing === "complete") return words.doneState;
  if (item.processing === "failed") return item.processing_error === "conversion_required" ? words.conversion : words.failedState;
  if (item.receipt !== "received") return words.receivingState;
  return words.receivedState;
}
function row(item) {
  const outer = document.createElement("div"); outer.className = "item";
  const left = document.createElement("div"); const right = document.createElement("div"); right.className = "right";
  const name = document.createElement("div"); name.className = "name"; name.textContent = item.filename;
  const source = document.createElement("div"); source.className = "source"; source.textContent = (item.senders || []).filter(Boolean).join(" · ");
  const status = document.createElement("div"); status.className = `state ${item.processing === "failed" ? "failed" : ""}`; status.textContent = stateLabel(item);
  const amount = document.createElement("div"); amount.className = "size"; amount.textContent = item.receipt === "received" ? size(item.total_bytes) : `${size(item.confirmed_bytes)} / ${size(item.total_bytes)}`;
  left.append(name,source); right.append(status,amount); outer.append(left,right); return outer;
}
async function loadHistory(append = false) {
  if (loading) return;
  loading = true; $("more").disabled = true;
  try {
    if (!append) { cursor = null; paged = false; $("items").replaceChildren(); }
    const params = new URLSearchParams({state:$("state-filter").value,kind:"all"});
    if (cursor != null) params.set("before", String(cursor));
    const page = await api(`/api/history?${params}`);
    for (const item of page.items) $("items").append(row(item));
    cursor = page.next_cursor;
    if (append) paged = true;
    $("more").hidden = cursor == null;
    message(page.items.length === 0 && !append ? words.empty : "");
  } catch (error) { if (error.message !== "login") message(words.connectionError); }
  finally { loading = false; $("more").disabled = false; }
}
async function overview() {
  if (!token) return;
  try {
    const value = await api("/api/overview");
    $("received").textContent = `${value.received} / ${value.total}`;
    $("published").textContent = String(value.published);
    $("failed").textContent = String(value.failed);
    $("reserved").textContent = size(value.reserved_bytes);
  } catch (error) { if (error.message !== "login") message(words.connectionError); }
}
async function show() { $("login-panel").hidden = true; $("dashboard").hidden = false; await Promise.all([overview(),loadHistory()]); }
$("login-form").addEventListener("submit", async event => {
  event.preventDefault(); const code = $("code").value.trim(); if (!/^[0-9]{10}$/.test(code)) return;
  try { const result = await api("/api/login", {method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({code})}); token = result.token; sessionStorage.setItem("photobridge-dashboard-token", token); $("code").value = ""; show(); }
  catch (error) { $("login-help").textContent = error.message === "429" ? words.locked : words.loginError; }
});
$("state-filter").addEventListener("change", () => loadHistory());
$("refresh").addEventListener("click", () => { overview(); loadHistory(); });
$("more").addEventListener("click", () => loadHistory(true));
$("retry").addEventListener("click", async () => {
  $("retry").disabled = true;
  try { const result = await api("/api/retry", {method:"POST"}); message(words.retried + result.count); await Promise.all([overview(),loadHistory()]); message(words.retried + result.count); }
  catch (error) { if (error.message !== "login") message(words.connectionError); }
  finally { $("retry").disabled = false; }
});
if (token) show();
setInterval(() => {
  if (!token || document.visibilityState !== "visible") return;
  overview();
  if (!paged) loadHistory();
}, 8000);
