"use strict";
const $ = id => document.getElementById(id);
const en = !navigator.language.toLowerCase().startsWith("zh");
const w = en ? {
  brand:"Receiver admin", section:"Workspace", navOverview:"Overview", navTransfers:"Transfers", local:"Private Wi-Fi only", breadcrumb:"Receiver", topLocal:"Local dashboard",
  loginTitle:"Connect to your Pixel", loginHelp:"Find the access code in PhotoBridge on your Pixel under Settings → Browser management. It changes each time you enable the page.", code:"10-digit access code", enter:"Open dashboard",
  eyebrow:"PIXEL RECEIVER", title:"Receiver overview", description:"Track files received, added to the phone gallery, and needing attention.", live:"Receiver online",
  received:"Received", receivedHint:"Confirmed on this Pixel", published:"In phone gallery", publishedHint:"Phone media processing complete", waiting:"Waiting", waitingHint:"Received, not yet in gallery", failed:"Failed", failedHint:"Retry from the list below",
  deviceTitle:"Phone status", deviceNote:"Live readings from the Pixel", temperatureLabel:"Battery temperature", batteryLabel:"Battery", freeLabel:"Available space", reservedLabel:"PhotoBridge originals", spaceDetail:(total,minimum)=>`Total ${size(total)} · minimum free ${size(minimum)}`, originalsDetail:"Excludes gallery copies", threshold:value=>`Battery pause at ${value} °C`, thermalPaused:"Heat pause active", thermalOff:"Temperature pause is off", waitingReading:"Waiting for reading", charging:"Charging", notCharging:"Not charging", transfers:"Transfer history", tableNote:"Open a file for full details.", retry:"Retry failed",
  guideTitle:"Status guide", receivingState:"Receiving", waitingState:"Waiting for gallery", failedState:"Gallery processing failed", publishedState:"In phone gallery", guideReceiving:"File is still transferring", guideWaiting:"Received; waiting to join the phone gallery", guidePublished:"In the Pixel gallery; cloud backup is unknown", guideFailed:"Originals remain on the receiver; retry is available",
  state:"Status", kind:"Type", allStates:"All statuses", receiving:"Receiving", processing:"Waiting", publishedFilter:"In gallery", failedFilter:"Failed", allKinds:"All types", photo:"Photo", video:"Video", motion:"Live", burst:"Burst", burstRole:"Burst role", burstPrimary:"Primary frame", burstOther:"Other frame", auto:"Auto refresh", refresh:"Refresh",
  file:"File", status:"Status", captured:"Captured", receivedTime:"Received", publishedTime:"Added to gallery", amount:"Size", perPage:"Per page", jump:"Go to", go:"Go", pagination:"Transfer pages", first:"First page", previous:"Previous page", next:"Next page", last:"Last page", page:n=>`Page ${n}`,
  count:n=>`${n} matching files`, updated:time=>`Updated ${time}`, olderPage:"This page does not auto-refresh", pageSummary:(page,pages,start,end,total)=>`Page ${page} of ${pages} · ${start}–${end} of ${total}`, unknown:"—", oldTime:"Not recorded", noCapture:"Unknown", noSender:"Unknown sender", noItems:"No transfers match these filters.",
  loginError:"Check the code on your phone and try again.", locked:"Too many attempts. Try again in five minutes.", connectionError:"Cannot reach your Pixel. Check that receiving and browser management are on.", retried:n=>`${n} failed item(s) queued for retry.`,
  detailTitle:"Transfer details", close:"Close details", senders:"Sending device", progress:"Receive progress", originals:"Originals", error:"Processing note", originalsReceiving:"Still receiving", originalsRetained:"Not cleared from the Pixel", originalsReleased:"Originals cleared from the receiver", conversion:"Enable compatibility conversion on the Pixel, then retry", unsupported:"This media format cannot be processed on the Pixel", retryHelp:"Try again on the Pixel; if it fails again, check receiver diagnostics", cloud:"Phone gallery completion does not confirm Google Photos cloud backup."
} : {
  brand:"接收端管理", section:"工作台", navOverview:"概览", navTransfers:"传输记录", local:"仅在当前局域网访问", breadcrumb:"接收端", topLocal:"本地管理页",
  loginTitle:"连接你的 Pixel", loginHelp:"在 Pixel 的 PhotoBridge「设置 → 浏览器管理」中查看访问码。每次开启都会更换。", code:"10 位访问码", enter:"进入管理页",
  eyebrow:"PIXEL RECEIVER", title:"接收概览", description:"查看接收、加入手机相册和待处理的文件。", live:"接收端在线",
  received:"已接收", receivedHint:"手机已确认收到的文件", published:"已加入相册", publishedHint:"手机媒体库处理完成", waiting:"等待处理", waitingHint:"已接收，尚未加入相册", failed:"处理失败", failedHint:"可在下方重试",
  deviceTitle:"手机状态", deviceNote:"来自接收端的实时读数", temperatureLabel:"电池温度", batteryLabel:"电量", freeLabel:"可用空间", reservedLabel:"PhotoBridge 原件", spaceDetail:(total,minimum)=>`总容量 ${size(total)} · 最低可用 ${size(minimum)}`, originalsDetail:"不含相册副本", threshold:value=>`电池达到 ${value} °C 暂停`, thermalPaused:"高温暂停接收", thermalOff:"温控已关闭", waitingReading:"等待读数", charging:"充电中", notCharging:"未充电", transfers:"传输记录", tableNote:"点击文件名查看完整信息。", retry:"重试失败项",
  guideTitle:"状态说明", receivingState:"接收中", waitingState:"等待处理", failedState:"处理失败", publishedState:"已加入相册", guideReceiving:"文件仍在传输", guideWaiting:"已收齐，等待加入手机相册", guidePublished:"已加入 Pixel 相册，云端备份状态未知", guideFailed:"原件仍在接收端，可重试",
  state:"状态", kind:"类型", allStates:"全部状态", receiving:"接收中", processing:"等待处理", publishedFilter:"已加入相册", failedFilter:"处理失败", allKinds:"全部类型", photo:"照片", video:"视频", motion:"实况", burst:"连拍", burstRole:"连拍角色", burstPrimary:"主照片", burstOther:"组内照片", auto:"自动刷新", refresh:"刷新",
  file:"文件", status:"状态", captured:"拍摄时间", receivedTime:"接收完成", publishedTime:"加入手机相册", amount:"大小", perPage:"每页", jump:"跳至", go:"前往", pagination:"传输记录分页", first:"第一页", previous:"上一页", next:"下一页", last:"最后一页", page:n=>`第 ${n} 页`,
  count:n=>`符合条件 ${n} 项`, updated:time=>`更新于 ${time}`, olderPage:"此页不自动刷新", pageSummary:(page,pages,start,end,total)=>`第 ${page} / ${pages} 页 · ${start}–${end} / ${total} 项`, unknown:"—", oldTime:"未记录", noCapture:"未知", noSender:"未知发送设备", noItems:"没有符合条件的传输记录。",
  loginError:"访问码不对，请查看手机后重试。", locked:"尝试次数过多，请五分钟后重试。", connectionError:"无法连接 Pixel，请确认接收和浏览器管理仍已开启。", retried:n=>`已将 ${n} 项失败记录重新排队。`,
  detailTitle:"传输详情", close:"关闭详情", senders:"发送设备", progress:"接收进度", originals:"原件状态", error:"处理提示", originalsReceiving:"仍在接收", originalsRetained:"未从 Pixel 清理", originalsReleased:"接收端原件已清理", conversion:"需在 Pixel 开启兼容转换后重试", unsupported:"Pixel 无法处理这种媒体格式", retryHelp:"请在 Pixel 重试；若再次失败，可查看接收端诊断信息", cloud:"加入手机相册不代表 Google 相册云端已备份。"
};
const textIds = {
  "brand-subtitle":"brand","side-section":"section","nav-overview":"navOverview","nav-transfers":"navTransfers","sidebar-local":"local","breadcrumb-current":"breadcrumb","top-local":"topLocal","login-title":"loginTitle","login-help":"loginHelp","code-label":"code","login-button":"enter","eyebrow":"eyebrow","page-title":"title","page-description":"description","live-text":"live",
  "label-received":"received","received-hint":"receivedHint","label-published":"published","published-hint":"publishedHint","label-waiting":"waiting","waiting-hint":"waitingHint","label-failed":"failed","failed-hint":"failedHint","device-title":"deviceTitle","device-note":"deviceNote","temperature-label":"temperatureLabel","battery-label":"batteryLabel","free-label":"freeLabel","reserved-label":"reservedLabel","transfers-title":"transfers","table-note":"tableNote","retry":"retry",
  "guide-title":"guideTitle","guide-receiving":"receivingState","guide-waiting":"waitingState","guide-published":"publishedState","guide-failed":"failedState","guide-receiving-help":"guideReceiving","guide-waiting-help":"guideWaiting","guide-published-help":"guidePublished","guide-failed-help":"guideFailed",
  "state-label":"state","kind-label":"kind","auto-label":"auto","refresh-label":"refresh","col-file":"file","col-status":"status","col-captured":"captured","col-size":"amount","per-page-label":"perPage","jump-label":"jump","jump-button":"go",
  "details-eyebrow":"detailTitle","dt-status":"status","dt-kind":"kind","dt-burst":"burstRole","dt-senders":"senders","dt-captured":"captured","dt-received":"receivedTime","dt-published":"publishedTime","dt-progress":"progress","dt-originals":"originals","dt-error":"error","details-cloud-note":"cloud"
};
for (const [id,key] of Object.entries(textIds)) $(id).textContent = w[key];
$("pagination").setAttribute("aria-label",w.pagination);
$("details-close").setAttribute("aria-label",w.close);
for (const [id,options] of Object.entries({"state-filter":{all:"allStates",receiving:"receiving",processing:"processing",published:"publishedFilter",failed:"failedFilter"},"kind-filter":{all:"allKinds",photo:"photo",video:"video",motion:"motion",burst:"burst"}})) {
  for (const [value,key] of Object.entries(options)) $(id).querySelector(`option[value="${value}"]`).textContent = w[key];
}
let token = sessionStorage.getItem("photobridge-dashboard-token") || "";
let currentPage = 1;
let totalPages = 1;
let historyVersion = 0;
let historyLoading = false;
let historyKey = "";
let paginationKey = "";
let overviewLoading = false;
const savedPageSize = localStorage.getItem("photobridge-dashboard-page-size");
$("per-page").value = ["20","50","100"].includes(savedPageSize) ? savedPageSize : "20";
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
  return Number.isNaN(value.getTime()) ? missing : dateFormat.format(value);
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
function processingNote(item) {
  if (item.processing !== "failed") return w.unknown;
  if (item.processing_error === "conversion_required") return w.conversion;
  if (item.processing_error === "unsupported") return w.unsupported;
  return w.retryHelp;
}
function kind(item) { return item.burst_primary == null || item.kind === "motion" ? item.kind : "burst"; }
function showDetails(item) {
  $("details-title").textContent = item.filename;
  $("detail-status").textContent = status(item).label;
  $("detail-kind").textContent = item.kind === "motion" && item.burst_primary != null ? `${w.motion} · ${w.burst}` : w[kind(item)] || w.unknown;
  $("dt-burst").hidden = item.burst_primary == null;
  $("detail-burst").hidden = item.burst_primary == null;
  $("detail-burst").textContent = item.burst_primary ? w.burstPrimary : w.burstOther;
  $("detail-senders").textContent = (item.senders || []).filter(Boolean).join(" · ") || w.noSender;
  $("detail-captured").textContent = date(item.captured_at_ms,w.noCapture);
  $("detail-received").textContent = date(item.received_at_ms,item.receipt === "received" ? w.oldTime : w.unknown);
  $("detail-published").textContent = date(item.published_at_ms,item.processing === "complete" ? w.oldTime : w.unknown);
  const total = Number(item.total_bytes) || 0, confirmed = Number(item.confirmed_bytes) || 0;
  $("detail-progress").textContent = `${size(confirmed)} / ${size(total)} (${total ? Math.min(100,Math.round(confirmed / total * 100)) : 0}%)`;
  $("detail-originals").textContent = item.receipt !== "received" ? w.originalsReceiving : item.originals_released ? w.originalsReleased : w.originalsRetained;
  $("detail-error").textContent = processingNote(item);
  $("details-dialog").showModal();
}
function cell(className, value) {
  const el = document.createElement("td"); el.className = className; el.textContent = value; return el;
}
function row(item) {
  const tr = document.createElement("tr");
  const file = document.createElement("td"); file.className = "file-cell";
  const wrap = document.createElement("div"); wrap.className = "file-wrap";
  const itemKind = kind(item);
  const icon = document.createElement("span"); icon.className = `file-icon ${itemKind}`; icon.textContent = itemKind === "video" ? "▶" : itemKind === "motion" ? "◉" : itemKind === "burst" ? "▦" : "▧"; icon.title = w[itemKind];
  const detail = document.createElement("div"); detail.className = "file-detail";
  const name = document.createElement("button"); name.type = "button"; name.className = "file-name"; name.textContent = item.filename; name.title = item.filename; name.addEventListener("click",() => showDetails(item));
  detail.append(name); wrap.append(icon,detail); file.append(wrap);
  const state = status(item);
  const statusCell = document.createElement("td");
  const badge = document.createElement("span"); badge.className = `status-badge ${state.key}`; badge.textContent = state.label;
  statusCell.append(badge);
  const amount = item.receipt === "received" ? size(item.total_bytes) : `${size(item.confirmed_bytes)} / ${size(item.total_bytes)}`;
  tr.append(file,statusCell,cell("date-cell",date(item.captured_at_ms,w.noCapture)),cell("size-cell",amount));
  return tr;
}
function pageButton(label,target,disabled,active,ariaLabel) {
  const button = document.createElement("button"); button.type = "button"; button.textContent = label; button.disabled = disabled; button.setAttribute("aria-label",ariaLabel);
  if (active) { button.className = "active"; button.setAttribute("aria-current","page"); }
  button.addEventListener("click",() => loadHistory(target));
  return button;
}
function renderPagination() {
  const key = `${currentPage}/${totalPages}`;
  if (key !== paginationKey) {
    const controls = [pageButton("«",1,currentPage === 1,false,w.first),pageButton("‹",currentPage-1,currentPage === 1,false,w.previous)];
    const visible = new Set([1,totalPages]);
    for (let page = Math.max(1,currentPage-2); page <= Math.min(totalPages,currentPage+2); page++) visible.add(page);
    let previous = 0;
    for (const page of [...visible].sort((a,b) => a-b)) {
      if (page-previous > 1) { const dots = document.createElement("span"); dots.className = "page-ellipsis"; dots.textContent = "…"; controls.push(dots); }
      controls.push(pageButton(String(page),page,page === currentPage,page === currentPage,w.page(page)));
      previous = page;
    }
    controls.push(pageButton("›",currentPage+1,currentPage === totalPages,false,w.next),pageButton("»",totalPages,currentPage === totalPages,false,w.last));
    $("pagination").replaceChildren(...controls); paginationKey = key;
  }
  $("jump-page").max = String(totalPages);
  if (document.activeElement !== $("jump-page")) $("jump-page").value = String(currentPage);
}
async function loadHistory(targetPage = currentPage) {
  const version = ++historyVersion;
  historyLoading = true;
  const perPage = Number($("per-page").value);
  const params = new URLSearchParams({state:$("state-filter").value,kind:$("kind-filter").value,page:String(targetPage),per_page:String(perPage)});
  try {
    const result = await api(`/api/history?${params}`);
    if (version !== historyVersion) return;
    const pages = Math.max(1,Number(result.pages) || 1);
    if (targetPage > pages) { await loadHistory(pages); return; }
    const key = JSON.stringify([params.toString(),result.items]);
    if (key !== historyKey) $("items").replaceChildren(...result.items.map(row));
    historyKey = key;
    currentPage = targetPage; totalPages = pages;
    $("result-count").textContent = w.count(result.total);
    $("updated-at").textContent = `${w.updated(timeFormat.format(new Date()))}${currentPage > 1 ? ` · ${w.olderPage}` : ""}`;
    const start = result.total ? (currentPage-1)*perPage+1 : 0;
    const end = Math.min(currentPage*perPage,result.total);
    $("page-summary").textContent = w.pageSummary(currentPage,totalPages,start,end,result.total);
    renderPagination();
    message(result.total === 0 ? w.noItems : "");
  } catch (error) { if (error.message !== "login") message(w.connectionError); }
  finally { if (version === historyVersion) historyLoading = false; }
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
    $("reserved-detail").textContent = w.originalsDetail;
    $("free").textContent = size(value.free_bytes);
    $("free-detail").textContent = w.spaceDetail(value.total_space_bytes,value.min_free_bytes);
    const device = value.device;
    $("temperature").textContent = device?.temperature_deci_celsius == null ? w.unknown : `${(device.temperature_deci_celsius / 10).toFixed(1)} °C`;
    $("thermal-state").textContent = !device ? w.waitingReading : device.thermal_held ? w.thermalPaused : device.thermal_enabled ? w.threshold(device.thermal_threshold_celsius) : w.thermalOff;
    $("thermal-state").classList.toggle("held",Boolean(device?.thermal_held));
    $("battery").textContent = device?.battery_percent == null ? w.unknown : `${device.battery_percent}%`;
    $("battery-state").textContent = device?.charging == null ? w.waitingReading : device.charging ? w.charging : w.notCharging;
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
$("state-filter").addEventListener("change",() => loadHistory(1));
$("kind-filter").addEventListener("change",() => loadHistory(1));
$("per-page").addEventListener("change",() => { localStorage.setItem("photobridge-dashboard-page-size",$("per-page").value); loadHistory(1); });
$("refresh").addEventListener("click",refreshAll);
$("jump-form").addEventListener("submit",event => {
  event.preventDefault();
  const page = Number($("jump-page").value);
  if (Number.isInteger(page) && page >= 1 && page <= totalPages) loadHistory(page);
});
$("details-close").addEventListener("click",() => $("details-dialog").close());
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
  if (currentPage === 1 && !historyLoading) loadHistory();
},8000);
