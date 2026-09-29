"use strict";
const $ = id => document.getElementById(id);
const en = !navigator.language.toLowerCase().startsWith("zh");
const w = en ? {
  brand:"Receiver admin", section:"Workspace", navOverview:"Overview", navTransfers:"Transfers", local:"Private Wi-Fi only", breadcrumb:"Receiver", topLocal:"Local dashboard", loginTitle:"Connect to your Pixel", loginHelp:"Find the access code in PhotoBridge on your Pixel under Settings → Browser management. It changes each time you enable the page.", code:"10-digit access code", enter:"Open dashboard", eyebrow:"PIXEL RECEIVER", title:"Receiver overview", description:"Track files received, added to the phone gallery, and needing attention.", live:"Receiver online", received:"Received", receivedHint:"Confirmed on this Pixel", published:"In phone gallery", publishedHint:"Phone media processing complete", waiting:"Waiting", waitingHint:"Received, not yet in gallery", failed:"Failed", failedHint:"Retry from the list below", storageTitle:"Receiver storage", reservedLabel:"Originals retained", storageDetail:(free,minimum)=>`${size(free)} free · ${size(minimum)} reserved minimum`, transfers:"Transfer history", cloud:"In phone gallery does not mean backed up to Google Photos cloud.", retry:"Retry failed", state:"Status", kind:"Type", allStates:"All statuses", receiving:"Receiving", processing:"Waiting", publishedFilter:"In gallery", failedFilter:"Failed", allKinds:"All types", photo:"Photo", video:"Video", motion:"Live photo", auto:"Auto refresh", refresh:"Refresh", file:"File", status:"Status", captured:"Captured", receivedTime:"Received", publishedTime:"Added to gallery", amount:"Size", footer:"Up to 50 items per page", more:"Load more", count:n=>`${n} matching files`, updated:time=>`Updated ${time}`, unknown:"—", oldTime:"Not recorded", noCapture:"Unknown", noSender:"Unknown sender", noItems:"No transfers match these filters.", loginError:"Check the code on your phone and try again.", locked:"Too many attempts. Try again in five minutes.", connectionError:"Cannot reach your Pixel. Check that receiving and browser management are on.", retried:n=>`${n} failed item(s) queued for retry.`, receivingState:"Receiving", waitingState:"Waiting for gallery", failedState:"Gallery processing failed", publishedState:"In phone gallery", conversion:"Enable compatibility conversion on the Pixel, then retry", listPaused:"List refresh pauses after loading more pages. Select Refresh to see the newest files."
} : {
  brand:"接收端管理", section:"工作台", navOverview:"概览", navTransfers:"传输记录", local:"仅在当前局域网访问", breadcrumb:"接收端", topLocal:"本地管理页", loginTitle:"连接你的 Pixel", loginHelp:"在 Pixel 的 PhotoBridge「设置 → 浏览器管理」中查看访问码。每次开启都会更换。", code:"10 位访问码", enter:"进入管理页", eyebrow:"PIXEL RECEIVER", title:"接收概览", description:"查看接收、加入手机相册和待处理的文件。", live:"接收端在线", received:"已接收", receivedHint:"手机已确认收到的文件", published:"已加入相册", publishedHint:"手机媒体库处理完成", waiting:"等待处理", waitingHint:"已接收，尚未加入相册", failed:"处理失败", failedHint:"可在下方重试", storageTitle:"接收端存储", reservedLabel:"保留原件", storageDetail:(free,minimum)=>`剩余 ${size(free)} · 最低预留 ${size(minimum)}`, transfers:"传输记录", cloud:"“已加入相册”指 Pixel 手机相册，不代表 Google 相册云端已备份。", retry:"重试失败项", state:"状态", kind:"类型", allStates:"全部状态", receiving:"接收中", processing:"等待处理", publishedFilter:"已加入相册", failedFilter:"处理失败", allKinds:"全部类型", photo:"照片", video:"视频", motion:"实况照片", auto:"自动刷新", refresh:"刷新", file:"文件", status:"状态", captured:"拍摄时间", receivedTime:"接收完成", publishedTime:"加入相册", amount:"大小", footer:"每次加载最多 50 项", more:"加载更多", count:n=>`符合条件 ${n} 项`, updated:time=>`更新于 ${time}`, unknown:"—", oldTime:"未记录", noCapture:"未知", noSender:"未知发送设备", noItems:"没有符合条件的传输记录。", loginError:"访问码不对，请查看手机后重试。", locked:"尝试次数过多，请五分钟后重试。", connectionError:"无法连接 Pixel，请确认接收和浏览器管理仍已开启。", retried:n=>`已将 ${n} 项失败记录重新排队。`, receivingState:"接收中", waitingState:"等待加入手机相册", failedState:"加入相册失败", publishedState:"已加入手机相册", conversion:"需在 Pixel 开启兼容转换后重试", listPaused:"加载更多后列表暂停自动刷新；点击刷新可查看最新记录。"
};
const textIds = {
  "brand-subtitle":"brand","side-section":"section","nav-overview":"navOverview","nav-transfers":"navTransfers","sidebar-local":"local","breadcrumb-current":"breadcrumb","top-local":"topLocal","login-title":"loginTitle","login-help":"loginHelp","code-label":"code","login-button":"enter","eyebrow":"eyebrow","page-title":"title","page-description":"description","live-text":"live","label-received":"received","received-hint":"receivedHint","label-published":"published","published-hint":"publishedHint","label-waiting":"waiting","waiting-hint":"waitingHint","label-failed":"failed","failed-hint":"failedHint","storage-title":"storageTitle","reserved-label":"reservedLabel","transfers-title":"transfers","cloud-note":"cloud","retry":"retry","state-label":"state","kind-label":"kind","auto-label":"auto","refresh-label":"refresh","col-file":"file","col-status":"status","col-captured":"captured","col-received":"receivedTime","col-published":"publishedTime","col-size":"amount","footer-note":"footer","more":"more"
};
for (const [id,key] of Object.entries(textIds)) $(id).textContent = w[key];
for (const [id,options] of Object.entries({"state-filter":{all:"allStates",receiving:"receiving",processing:"processing",published:"publishedFilter",failed:"failedFilter"},"kind-filter":{all:"allKinds",photo:"photo",video:"video",motion:"motion"}})) {
  for (const [value,key] of Object.entries(options)) $(id).querySelector(`option[value="${value}"]`).textContent = w[key];
}
let token = sessionStorage.getItem("photobridge-dashboard-token") || "";
let cursor = null;
let paged = false;
let historyVersion = 0;
let historyKey = "";
let overviewLoading = false;
const dateFormat = new Intl.DateTimeFormat(en ? "en" : "zh-CN", {year:"numeric",month:"2-digit",day:"2-digit",hour:"2-digit",minute:"2-digit"});
const timeFormat = new Intl.DateTimeFormat(en ? "en" : "zh-CN", {hour:"2-digit",minute:"2-digit",second:"2-digit"});
function size(bytes) {
  if (bytes == null || !Number.isFinite(Number(bytes))) return w.unknown;
  const value = Number(bytes);
  if (value < 1024) return `${value} B`;
  const units = ["KB","MB","GB","TB"];
  let number = value / 1024, index = 0;
  while (number >= 1024 && index < units.length - 1) { number /= 1024; index++; }
  return `${number.toFixed(number >= 10 ? 0 : 1)} ${units[index]}`;
}
function date(ms, missing = w.oldTime) {
  if (!Number.isFinite(ms) || ms <= 0) return missing;
  const value = new Date(ms);
  return Number.isNaN(value.getTime()) ? w.oldTime : dateFormat.format(value);
}
function message(value) { $("message").textContent = value; $("message").hidden = !value; }
async function api(path, options = {}) {
  const headers = {...(options.headers || {})};
  if (token) headers.Authorization = `Bearer ${token}`;
  const response = await fetch(path, {...options, headers, cache:"no-store"});
  if (response.status === 401 && path !== "/api/login") {
    token = ""; sessionStorage.removeItem("photobridge-dashboard-token");
    $("login-panel").hidden = false; $("dashboard").hidden = true;
    throw new Error("login");
  }
  if (!response.ok) throw new Error(String(response.status));
  return response.json();
}
function status(item) {
  if (item.receipt !== "received") return {key:"receiving",label:w.receivingState};
  if (item.processing === "complete") return {key:"published",label:w.publishedState};
  if (item.processing === "failed") return {key:"failed",label:w.failedState};
  return {key:"waiting",label:w.waitingState};
}
function cell(className, value) {
  const el = document.createElement("td"); el.className = className; el.textContent = value; return el;
}
function row(item) {
  const tr = document.createElement("tr");
  const file = document.createElement("td"); file.className = "file-cell";
  const wrap = document.createElement("div"); wrap.className = "file-wrap";
  const icon = document.createElement("span"); icon.className = `file-icon ${item.kind}`; icon.textContent = item.kind === "video" ? "▶" : item.kind === "motion" ? "◉" : "▧";
  const detail = document.createElement("div"); detail.className = "file-detail";
  const name = document.createElement("strong"); name.textContent = item.filename;
  const sub = document.createElement("small"); sub.textContent = (item.senders || []).filter(Boolean).join(" · ") || w.noSender;
  detail.append(name, sub); wrap.append(icon, detail); file.append(wrap);
  const state = status(item);
  const statusCell = document.createElement("td");
  const badge = document.createElement("span"); badge.className = `status-badge ${state.key}`; badge.textContent = state.label;
  statusCell.append(badge);
  if (item.processing_error === "conversion_required") { const hint = document.createElement("small"); hint.className = "status-hint"; hint.textContent = w.conversion; statusCell.append(hint); }
  const amount = item.receipt === "received" ? size(item.total_bytes) : `${size(item.confirmed_bytes)} / ${size(item.total_bytes)}`;
  tr.append(file,statusCell,cell("date-cell",date(item.captured_at_ms,w.noCapture)),cell("date-cell",date(item.received_at_ms,item.receipt === "received" ? w.oldTime : w.unknown)),cell("date-cell",date(item.published_at_ms,item.processing === "complete" ? w.oldTime : w.unknown)),cell("size-cell",amount));
  return tr;
}
async function loadHistory(append = false) {
  const version = ++historyVersion;
  $("more").disabled = true;
  const currentCursor = append ? cursor : null;
  const params = new URLSearchParams({state:$("state-filter").value,kind:$("kind-filter").value});
  if (currentCursor != null) params.set("before",String(currentCursor));
  try {
    const page = await api(`/api/history?${params}`);
    if (version !== historyVersion) return;
    const key = JSON.stringify([params.get("state"),params.get("kind"),page.items,page.next_cursor]);
    if (append) { $("items").append(...page.items.map(row)); paged = true; historyKey = ""; }
    else {
      if (paged || key !== historyKey) $("items").replaceChildren(...page.items.map(row));
      paged = false; historyKey = key;
    }
    cursor = page.next_cursor;
    $("more").hidden = cursor == null;
    $("result-count").textContent = w.count(page.total);
    $("updated-at").textContent = w.updated(timeFormat.format(new Date()));
    message(page.total === 0 ? w.noItems : paged ? w.listPaused : "");
  } catch (error) { if (error.message !== "login") message(w.connectionError); }
  finally { if (version === historyVersion) $("more").disabled = false; }
}
async function overview() {
  if (!token || overviewLoading) return;
  overviewLoading = true;
  try {
    const value = await api("/api/overview");
    $("received").textContent = `${value.received} / ${value.total}`;
    $("published").textContent = String(value.published);
    $("waiting").textContent = String(value.waiting);
    $("failed").textContent = String(value.failed);
    $("reserved").textContent = size(value.reserved_bytes);
    $("storage-detail").textContent = w.storageDetail(value.free_bytes,value.min_free_bytes);
    $("live-text").textContent = w.live;
    $("live-text").parentElement.classList.remove("offline");
  } catch (error) {
    if (error.message !== "login") { message(w.connectionError); $("live-text").textContent = w.connectionError; $("live-text").parentElement.classList.add("offline"); }
  } finally { overviewLoading = false; }
}
async function refreshAll() { await Promise.all([overview(),loadHistory()]); }
async function show() { $("login-panel").hidden = true; $("dashboard").hidden = false; await refreshAll(); }
$("login-form").addEventListener("submit",async event => {
  event.preventDefault();
  const code = $("code").value.trim(); if (!/^[0-9]{10}$/.test(code)) return;
  try {
    const result = await api("/api/login",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({code})});
    token = result.token; sessionStorage.setItem("photobridge-dashboard-token",token); $("code").value = ""; show();
  } catch (error) { $("login-help").textContent = error.message === "429" ? w.locked : w.loginError; }
});
$("state-filter").addEventListener("change",() => loadHistory());
$("kind-filter").addEventListener("change",() => loadHistory());
$("refresh").addEventListener("click",refreshAll);
$("more").addEventListener("click",() => loadHistory(true));
$("retry").addEventListener("click",async () => {
  $("retry").disabled = true;
  try { const result = await api("/api/retry",{method:"POST"}); await refreshAll(); message(w.retried(result.count)); }
  catch (error) { if (error.message !== "login") message(w.connectionError); }
  finally { $("retry").disabled = false; }
});
$("auto-refresh").checked = localStorage.getItem("photobridge-dashboard-auto-refresh") !== "off";
$("auto-refresh").addEventListener("change",() => localStorage.setItem("photobridge-dashboard-auto-refresh",$("auto-refresh").checked ? "on" : "off"));
if (token) show();
setInterval(() => {
  if (!token || !$("auto-refresh").checked || document.visibilityState !== "visible") return;
  overview();
  if (!paged && !$("more").disabled) loadHistory();
},8000);
