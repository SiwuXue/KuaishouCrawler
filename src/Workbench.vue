<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  Activity,
  ArrowDownToLine,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleHelp,
  Clock3,
  Database,
  Download,
  FileJson,
  FileSpreadsheet,
  FolderOpen,
  History,
  MessageSquareText,
  Play,
  Plus,
  Search,
  Settings,
  ShieldCheck,
  SlidersHorizontal,
  Square,
  Terminal,
  Trash2,
  UserRound,
  Video,
  X,
  Zap,
} from "lucide-vue-next";

type Page = "task" | "files" | "logs" | "settings";
type Mode = "search" | "detail" | "creator";
type LogLevel = "success" | "info" | "warning" | "error" | "debug";
type SaveFormat = "jsonl" | "json" | "csv" | "excel";
interface LogLine { id: number | string; time: string; level: LogLevel; message: string }
interface WorkbenchSettings { defaultFormat: SaveFormat; outputDirectory: string }
interface DataFile {
  id: number | string;
  name: string;
  path: string;
  mode: string;
  type: "JSONL" | "JSON" | "CSV" | "XLSX" | "XLS";
  records: number | null;
  size: number;
  updated: string;
  columns: string[];
  preview: Record<string, unknown>[];
}
interface EngineStatus { ready: boolean; pythonInstalled: boolean; browserInstalled: boolean; engineInstalled: boolean; message: string }
interface TaskStatus { status: "idle" | "running" | "stopping" | "error"; kind: string; target: string; startedAt: number | null }
interface EngineLog { id: number | string; timestamp: string; level: LogLevel; message: string }
interface FilePreview { data: Record<string, unknown>[]; total: number; columns: string[] }

const page = ref<Page>("task");
const sidebarStorageKey = "kuaishou-workbench-sidebar-collapsed";
const settingsStorageKey = "kuaishou-workbench-settings";
const sidebarCollapsed = ref(readSidebarCollapsed());
const compactViewport = ref(window.innerWidth <= 1020);
const mode = ref<Mode>("search");
const keywordInput = ref("");
const detailInput = ref("");
const creatorInput = ref("");
const startPage = ref(1);
const maxItems = ref(10);
const maxComments = ref(20);
const settings = ref(readSettings());
const saveFormat = ref<SaveFormat>(settings.value.defaultFormat);
const includeComments = ref(true);
const includeReplies = ref(false);
const advancedOpen = ref(false);
const isRunning = ref(false);
const isStopping = ref(false);
const isSetupRunning = ref(false);
const engineStatus = ref<EngineStatus>({ ready: false, pythonInstalled: false, browserInstalled: false, engineInstalled: true, message: "正在检查本机运行环境" });
const taskStatus = ref<TaskStatus>({ status: "idle", kind: "", target: "", startedAt: null });
const activeTask = ref("");
const toast = ref("");
const toastTimer = ref<number | undefined>();
const logClock = ref(0);
const selectedFile = ref<DataFile | null>(null);
const previewLoading = ref(false);
const filesLoading = ref(false);
const searchFiles = ref("");
const fileTypeFilter = ref("全部格式");
const logSearch = ref("");
const logLevelFilter = ref<"all" | LogLevel>("all");
const eventCleanups: UnlistenFn[] = [];

function readSettings(): WorkbenchSettings {
  const defaults: WorkbenchSettings = { defaultFormat: "jsonl", outputDirectory: "data/ks/" };
  try {
    const stored = window.localStorage.getItem(settingsStorageKey);
    if (!stored) return defaults;
    const parsed = JSON.parse(stored) as Partial<WorkbenchSettings>;
    const validFormats: SaveFormat[] = ["jsonl", "json", "csv", "excel"];
    return {
      defaultFormat: validFormats.includes(parsed.defaultFormat as SaveFormat) ? parsed.defaultFormat as SaveFormat : defaults.defaultFormat,
      outputDirectory: typeof parsed.outputDirectory === "string" ? parsed.outputDirectory : defaults.outputDirectory,
    };
  } catch {
    return defaults;
  }
}

const logLines = ref<LogLine[]>([]);

function readSidebarCollapsed() {
  try {
    return window.localStorage.getItem(sidebarStorageKey) === "true";
  } catch {
    return false;
  }
}

watch(sidebarCollapsed, (collapsed) => {
  try {
    window.localStorage.setItem(sidebarStorageKey, String(collapsed));
  } catch {
    // The current session still works when browser storage is unavailable.
  }
});

watch(settings, (value) => {
  try {
    window.localStorage.setItem(settingsStorageKey, JSON.stringify(value));
  } catch {
    // Settings remain usable for this session when browser storage is unavailable.
  }
}, { deep: true });

const files = ref<DataFile[]>([]);

function updateCompactViewport() {
  compactViewport.value = window.innerWidth <= 1020;
}

const visibleFiles = computed(() => {
  const query = searchFiles.value.trim().toLowerCase();
  return files.value.filter((file) => {
    const matchesQuery = !query || file.name.toLowerCase().includes(query) || file.mode.includes(query);
    return matchesQuery && (fileTypeFilter.value === "全部格式" || file.type === fileTypeFilter.value);
  });
});
const pageTitle = computed(() => ({ task: "采集任务", files: "数据文件", logs: "运行日志", settings: "设置" })[page.value]);
const visibleLogs = computed(() => {
  const query = logSearch.value.trim().toLocaleLowerCase();
  return [...logLines.value].reverse().filter((line) => {
    const matchesQuery = !query || line.message.toLocaleLowerCase().includes(query) || line.time.includes(query);
    return matchesQuery && (logLevelFilter.value === "all" || line.level === logLevelFilter.value);
  });
});
const recordCount = computed(() => files.value.reduce((sum, file) => sum + (file.records ?? 0), 0));
const storageSize = computed(() => files.value.reduce((sum, file) => sum + file.size, 0));
const modeName = computed(() => ({ search: "关键词搜索", detail: "视频详情", creator: "创作者主页" })[mode.value]);
const taskTarget = computed(() => mode.value === "search" ? keywordInput.value.trim() : mode.value === "detail" ? detailInput.value.trim() : creatorInput.value.trim());
const formatName = (format: SaveFormat) => ({ jsonl: "JSONL", json: "JSON", csv: "CSV", excel: "Excel" })[format];

function notify(message: string) {
  toast.value = message;
  window.clearTimeout(toastTimer.value);
  toastTimer.value = window.setTimeout(() => (toast.value = ""), 2600);
}

function addLog(level: LogLevel, message: string) {
  logClock.value += 1;
  logLines.value.push({ id: Date.now() + logClock.value, time: new Date().toLocaleTimeString("zh-CN", { hour12: false }), level, message });
  if (logLines.value.length > 500) logLines.value = logLines.value.slice(-500);
}

function updateTaskStatus(status: TaskStatus) {
  taskStatus.value = status;
  isRunning.value = status.kind === "crawler" && (status.status === "running" || status.status === "stopping");
  isStopping.value = status.kind === "crawler" && status.status === "stopping";
  isSetupRunning.value = status.kind === "setup" && status.status === "running";
  if (status.target) activeTask.value = status.target;
  if (status.status === "idle" || status.status === "error") {
    window.setTimeout(() => void refreshFiles(), 500);
  }
}

async function loadEngineStatus() {
  engineStatus.value = await invoke<EngineStatus>("get_engine_status");
}

async function refreshFiles() {
  if (!engineStatus.value.pythonInstalled || filesLoading.value) return;
  filesLoading.value = true;
  try {
    const result = await invoke<{ files: Omit<DataFile, "columns">[] }>("list_data_files", { outputDirectory: settings.value.outputDirectory });
    files.value = result.files.map((file) => ({ ...file, columns: [], preview: [] }));
  } catch (error) {
    notify(`读取文件列表失败：${String(error)}`);
  } finally {
    filesLoading.value = false;
  }
}

watch(page, (nextPage) => {
  if (nextPage === "files") void refreshFiles();
});

async function runCrawler() {
  if (isRunning.value) {
    if (isStopping.value) return;
    try {
      await invoke("stop_crawler");
      isStopping.value = true;
      notify("正在停止采集任务");
    } catch (error) {
      notify(String(error));
    }
    return;
  }
  if (isSetupRunning.value) return notify("运行环境正在安装，请稍后再试");
  if (!taskTarget.value) {
    notify(mode.value === "search" ? "先输入要搜索的关键词" : "请填写采集目标");
    return;
  }
  if (!engineStatus.value.ready) {
    page.value = "settings";
    notify("请先在设置页安装本机采集运行环境");
    return;
  }

  activeTask.value = taskTarget.value;
  try {
    await invoke("start_crawler", {
      request: {
        mode: mode.value,
        target: taskTarget.value,
        startPage: Math.max(1, Number(startPage.value) || 1),
        maxItems: Math.max(1, Number(maxItems.value) || 1),
        maxComments: Math.max(1, Number(maxComments.value) || 1),
        includeComments: includeComments.value,
        includeReplies: includeReplies.value,
        saveFormat: saveFormat.value,
      },
      outputDirectory: settings.value.outputDirectory,
    });
    isRunning.value = true;
    isStopping.value = false;
    notify("采集任务已启动");
  } catch (error) {
    isRunning.value = false;
    addLog("error", `任务启动失败：${String(error)}`);
    notify(String(error));
  }
}

function resetTask() {
  if (isRunning.value || isSetupRunning.value) return notify("当前任务结束后再新建任务");
  mode.value = "search";
  keywordInput.value = "";
  detailInput.value = "";
  creatorInput.value = "";
  startPage.value = 1;
  maxItems.value = 10;
  saveFormat.value = settings.value.defaultFormat;
  addLog("info", `已新建任务，默认保存格式：${formatName(settings.value.defaultFormat)}`);
  notify("已准备好新任务");
}

function clearLogs() {
  logLines.value = [];
  notify("运行日志已清空");
}

function formatSize(bytes: number) {
  return bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

async function setupBackend() {
  if (isRunning.value) return notify("请先停止当前采集任务");
  try {
    await invoke("setup_backend");
    isSetupRunning.value = true;
    page.value = "logs";
    addLog("info", "开始安装项目专用 Python 和 Chromium 运行环境");
    notify("正在安装运行环境，请稍候");
  } catch (error) {
    notify(String(error));
  }
}

async function selectOutputDirectory() {
  try {
    const selected = await open({ directory: true, multiple: false, title: "选择采集文件保存目录" });
    if (typeof selected === "string") settings.value.outputDirectory = selected;
  } catch (error) {
    notify(`无法打开文件夹选择器：${String(error)}`);
  }
}

async function openPreview(file: DataFile) {
  selectedFile.value = file;
  previewLoading.value = true;
  try {
    const preview = await invoke<FilePreview>("preview_data_file", {
      outputDirectory: settings.value.outputDirectory,
      relativePath: file.path,
    });
    selectedFile.value = { ...file, columns: preview.columns, preview: preview.data, records: preview.total };
  } catch (error) {
    notify(`文件预览失败：${String(error)}`);
  } finally {
    previewLoading.value = false;
  }
}

async function exportFile(file: DataFile) {
  try {
    const destination = await save({
      defaultPath: file.name,
      filters: [{ name: "采集文件", extensions: [file.type.toLowerCase()] }],
    });
    if (!destination) return;
    await invoke("export_data_file", {
      outputDirectory: settings.value.outputDirectory,
      relativePath: file.path,
      destination,
    });
    notify("文件已导出");
  } catch (error) {
    notify(`导出失败：${String(error)}`);
  }
}

function downloadSelected() { if (selectedFile.value) void exportFile(selectedFile.value); }
function formatCell(value: unknown) {
  if (value === null || value === undefined || value === "") return "—";
  return typeof value === "object" ? JSON.stringify(value) : String(value);
}
function openHelp() { notify("启动任务后会打开本机浏览器，请在快手官方页面自行完成登录和确认。"); }
function closePreview() { selectedFile.value = null; }

onMounted(async () => {
  window.addEventListener("resize", updateCompactViewport);
  eventCleanups.push(await listen<EngineLog>("crawler-log", (event) => {
    const line = event.payload;
    logClock.value += 1;
    const time = new Date(Number(line.timestamp) * 1000).toLocaleTimeString("zh-CN", { hour12: false });
    logLines.value.push({ id: String(line.id || Date.now() + logClock.value), time, level: line.level, message: line.message });
    if (logLines.value.length > 500) logLines.value = logLines.value.slice(-500);
  }));
  eventCleanups.push(await listen<TaskStatus>("crawler-status", (event) => updateTaskStatus(event.payload)));
  eventCleanups.push(await listen<EngineStatus>("engine-status", (event) => {
    engineStatus.value = event.payload;
    if (event.payload.ready) {
      addLog("success", "本机采集运行环境已就绪");
      void refreshFiles();
    }
  }));
  try {
    await loadEngineStatus();
    const status = await invoke<TaskStatus>("get_task_status");
    updateTaskStatus(status);
    addLog(engineStatus.value.ready ? "success" : "warning", engineStatus.value.message);
    if (engineStatus.value.pythonInstalled) await refreshFiles();
  } catch (error) {
    engineStatus.value = { ready: false, pythonInstalled: false, browserInstalled: false, engineInstalled: false, message: "桌面原生命令未连接" };
    addLog("error", `无法连接本机运行环境：${String(error)}`);
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", updateCompactViewport);
  eventCleanups.forEach((cleanup) => cleanup());
  window.clearTimeout(toastTimer.value);
});
</script>

<template>
  <div class="app-shell">
    <aside :class="['sidebar', { 'sidebar--collapsed': sidebarCollapsed }]">
      <div class="brand-lockup">
        <div class="brand-mark" role="img" aria-label="快手采集工作台" title="快手采集工作台"><span></span><i></i></div>
        <div class="brand-copy"><strong>快手采集</strong><span>本地工作空间</span></div>
        <button class="sidebar-toggle" type="button" :aria-label="sidebarCollapsed ? '展开侧栏' : '收起侧栏'" :title="sidebarCollapsed ? '展开侧栏' : '收起侧栏'" :aria-expanded="!sidebarCollapsed" aria-controls="main-navigation" @click="sidebarCollapsed = !sidebarCollapsed">
          <ChevronRight v-if="sidebarCollapsed" :size="16" />
          <ChevronLeft v-else :size="16" />
        </button>
      </div>

      <div class="side-caption">工作台</div>
      <nav id="main-navigation" class="side-nav" aria-label="主导航">
        <button :class="['nav-item', { 'nav-item--active': page === 'task' }]" type="button" aria-label="快手采集" :title="sidebarCollapsed || compactViewport ? '快手采集' : undefined" @click="page = 'task'">
          <Activity :size="18" :stroke-width="1.7" /><span>快手采集</span>
        </button>
        <button :class="['nav-item', { 'nav-item--active': page === 'files' }]" type="button" aria-label="数据文件" :title="sidebarCollapsed || compactViewport ? '数据文件' : undefined" @click="page = 'files'">
          <FolderOpen :size="18" :stroke-width="1.7" /><span>数据文件</span><small>{{ files.length }}</small>
        </button>
        <button :class="['nav-item', { 'nav-item--active': page === 'logs' }]" type="button" aria-label="运行日志" :title="sidebarCollapsed || compactViewport ? '运行日志' : undefined" @click="page = 'logs'">
          <Terminal :size="18" :stroke-width="1.7" /><span>运行日志</span><small>{{ logLines.length }}</small>
        </button>
      </nav>

      <div class="sidebar-bottom">
        <div class="local-card" title="数据留在本机"><ShieldCheck :size="17" /><span><b>数据留在本机</b><small>本地工作空间</small></span></div>
        <button :class="['sidebar-settings', 'nav-item', { 'nav-item--active': page === 'settings' }]" type="button" aria-label="设置" :title="sidebarCollapsed || compactViewport ? '设置' : undefined" @click="page = 'settings'">
          <Settings :size="17" :stroke-width="1.7" /><span>设置</span>
        </button>
        <button class="sidebar-help" type="button" aria-label="登录与使用说明" title="登录与使用说明" @click="openHelp"><CircleHelp :size="16" /><span>登录与使用说明</span></button>
      </div>
    </aside>

    <main class="main-area">
      <header class="topbar">
        <div class="crumb"><span>快手</span><span class="crumb-divider">/</span><b>{{ pageTitle }}</b></div>
        <div class="topbar-right"><span :class="['demo-pill', { 'demo-pill--ready': engineStatus.ready }]" :title="engineStatus.message"><i></i>{{ engineStatus.ready ? '本地引擎已就绪' : isSetupRunning ? '正在安装运行环境' : '运行环境未就绪' }}</span><span class="offline-note"><span></span>{{ isRunning ? (isStopping ? '正在停止任务' : '正在采集') : engineStatus.message }}</span></div>
      </header>

      <div class="page-content">
        <template v-if="page === 'task'">
          <section class="page-intro">
            <div>
              <div class="eyebrow"><span></span> KUAISHOU <b>/</b> WORKSPACE</div>
              <h1>快手采集</h1>
              <p>设置采集范围并启动本机任务，结果会保存到你选择的本地目录。</p>
            </div>
            <button class="quiet-button" type="button" @click="resetTask"><Plus :size="16" />新建任务</button>
          </section>

          <div class="scope-line">
            <span><i class="scope-check"><Check :size="11" /></i>公开内容</span><span class="scope-separator">·</span>
            <span><i class="scope-check"><Check :size="11" /></i>本地保存</span><span class="scope-separator">·</span>
            <span><i class="scope-check"><Check :size="11" /></i>数量可控</span>
            <span class="scope-demo">本地文件：{{ files.length }} 个 · {{ recordCount }} 条记录</span>
          </div>

          <div class="workspace-grid">
            <section class="task-card">
              <header class="section-heading">
                <div><span class="section-kicker">新建任务</span><h2>你想采集什么？</h2><p>选择一种方式，设置本次采集的内容范围。</p></div>
                <span class="step-label">01 <i>/</i> 任务配置</span>
              </header>

              <div class="form-section">
                <label class="input-label">采集方式</label>
                <div class="mode-picker" role="tablist" aria-label="采集方式">
                  <button :class="['mode-option', { selected: mode === 'search' }]" type="button" role="tab" :aria-selected="mode === 'search'" @click="mode = 'search'"><Search :size="16" /><span><b>关键词搜索</b><small>发现相关作品</small></span></button>
                  <button :class="['mode-option', { selected: mode === 'detail' }]" type="button" role="tab" :aria-selected="mode === 'detail'" @click="mode = 'detail'"><Video :size="16" /><span><b>视频详情</b><small>指定作品链接</small></span></button>
                  <button :class="['mode-option', { selected: mode === 'creator' }]" type="button" role="tab" :aria-selected="mode === 'creator'" @click="mode = 'creator'"><UserRound :size="16" /><span><b>创作者主页</b><small>跟进公开作品</small></span></button>
                </div>
              </div>

              <div class="form-section target-section">
                <label v-if="mode === 'search'" class="input-label" for="keyword">搜索关键词 <span>*</span></label>
                <label v-else-if="mode === 'detail'" class="input-label" for="detail">视频链接或作品 ID <span>*</span></label>
                <label v-else class="input-label" for="creator">创作者主页或 ID <span>*</span></label>
                <div v-if="mode === 'search'" class="text-field"><Search :size="17" /><input id="keyword" v-model="keywordInput" type="text" placeholder="试试：成都咖啡探店" @keydown.enter="runCrawler" /></div>
                <textarea v-else-if="mode === 'detail'" id="detail" v-model="detailInput" class="text-area" rows="2" placeholder="粘贴快手视频链接或作品 ID，每行一个"></textarea>
                <textarea v-else id="creator" v-model="creatorInput" class="text-area" rows="2" placeholder="粘贴创作者主页链接或用户 ID"></textarea>
                <p class="field-note">{{ mode === 'search' ? '多个关键词请使用英文逗号分隔。' : mode === 'detail' ? '支持完整作品链接或纯 ID，最多 20 个。' : '支持创作者主页链接或纯用户 ID。' }}</p>
              </div>

              <div class="form-section range-section">
                <div class="range-field"><label class="input-label" for="start-page">从第几页开始</label><div class="number-field"><input id="start-page" v-model.number="startPage" type="number" min="1" max="999" /><span>页</span><div class="number-actions"><button type="button" aria-label="增加页码" @click="startPage = Math.min(startPage + 1, 999)"><ChevronDown class="arrow-up" :size="14" /></button><button type="button" aria-label="减少页码" @click="startPage = Math.max(startPage - 1, 1)"><ChevronDown :size="14" /></button></div></div></div>
                <div class="range-field"><label class="input-label" for="max-items">最多采集</label><div class="number-field"><input id="max-items" v-model.number="maxItems" type="number" min="1" max="100" /><span>条作品</span><div class="number-actions"><button type="button" aria-label="增加数量" @click="maxItems = Math.min(maxItems + 1, 100)"><ChevronDown class="arrow-up" :size="14" /></button><button type="button" aria-label="减少数量" @click="maxItems = Math.max(maxItems - 1, 1)"><ChevronDown :size="14" /></button></div></div></div>
              </div>

              <button class="advanced-toggle" type="button" :aria-expanded="advancedOpen" @click="advancedOpen = !advancedOpen"><SlidersHorizontal :size="15" />更多采集选项<ChevronDown :class="{ expanded: advancedOpen }" :size="14" /></button>
              <div v-if="advancedOpen" class="advanced-panel">
                <div class="option-line"><span class="option-icon"><MessageSquareText :size="16" /></span><div class="option-text"><b>采集一级评论</b><small>获取作品下的评论内容</small></div><button :class="['switch', { on: includeComments }]" type="button" role="switch" :aria-checked="includeComments" aria-label="采集一级评论" @click="includeComments = !includeComments"><i></i></button></div>
                <div v-if="includeComments" class="comment-limit"><span>每个作品最多采集 {{ maxComments }} 条评论</span><div class="inline-stepper"><button type="button" aria-label="减少评论数量" @click="maxComments = Math.max(1, maxComments - 5)">−</button><button type="button" aria-label="增加评论数量" @click="maxComments = Math.min(500, maxComments + 5)">+</button></div></div>
                <div class="option-line"><span class="option-icon option-icon--quiet"><MessageSquareText :size="16" /></span><div class="option-text"><b>采集二级评论</b><small>读取评论下的回复</small></div><button :class="['switch', { on: includeReplies }]" type="button" role="switch" :aria-checked="includeReplies" aria-label="采集二级评论" @click="includeReplies = !includeReplies"><i></i></button></div>
                <label class="format-line"><span>保存格式</span><div class="format-control"><component :is="saveFormat === 'csv' || saveFormat === 'excel' ? FileSpreadsheet : FileJson" :size="16" /><select v-model="saveFormat"><option value="jsonl">JSONL 文件</option><option value="json">JSON 文件</option><option value="csv">CSV 表格</option><option value="excel">Excel 表格</option></select><ChevronDown :size="14" /></div></label>
              </div>

              <div class="login-note"><span class="login-symbol"><Zap :size="17" /></span><p><b>需要登录快手？</b>启动后会打开本机浏览器，请在官方页面自行完成登录与确认。</p><button type="button" aria-label="登录说明" @click="openHelp"><CircleHelp :size="16" /></button></div>

              <div class="task-footer"><span><ShieldCheck :size="16" />按需采集，数据保存在本机</span><button :class="['run-button', { stopping: isRunning }]" type="button" :disabled="isStopping || isSetupRunning" @click="runCrawler"><Square v-if="isRunning" :size="15" fill="currentColor" /><Play v-else :size="15" fill="currentColor" />{{ isStopping ? '正在停止' : isRunning ? '停止任务' : '开始采集' }}</button></div>
            </section>

            <aside class="activity-column">
              <section class="activity-card status-card">
                <div class="aside-heading"><div><span class="section-kicker">实时进度</span><h2>任务状态</h2></div><span :class="['state-chip', { active: isRunning }]"><i></i>{{ isRunning ? '正在运行' : '等待开始' }}</span></div>
                <div class="status-content">
                  <div class="status-mark" :class="{ pulsing: isRunning || isSetupRunning }"><Activity :size="23" /></div>
                  <div class="status-description"><b>{{ isSetupRunning ? '正在准备运行环境' : isRunning ? modeName : taskStatus.status === 'error' ? '上次任务异常结束' : '准备好开始采集' }}</b><span>{{ isRunning ? activeTask : isSetupRunning ? 'Python 与浏览器安装进度请查看运行日志。' : engineStatus.message }}</span></div>
                </div>
                <div class="progress-copy"><span>{{ isRunning ? '任务状态' : '采集引擎' }}</span><b>{{ isRunning ? (isStopping ? '停止请求已发送' : '运行中 · 数量上限 ' + maxItems) : engineStatus.ready ? '本机就绪' : '待安装' }}</b></div>
                <div :class="['progress-track', { 'progress-track--active': isRunning || isSetupRunning }]"><i></i></div>
                <div class="status-foot"><span><Clock3 :size="14" />{{ isRunning ? '本机采集任务' : taskStatus.startedAt ? `最近启动：${new Date(taskStatus.startedAt * 1000).toLocaleTimeString('zh-CN', { hour12: false })}` : '尚未启动任务' }}</span><span v-if="isRunning || isSetupRunning" class="live-label"><i></i>实时</span><span v-else>本地运行</span></div>
              </section>

              <section class="activity-card log-card">
                <div class="aside-heading log-heading"><div><span class="section-kicker">TASK ACTIVITY</span><h2><Terminal :size="16" />运行日志 <small>{{ logLines.length }}</small></h2></div><button class="text-action" type="button" @click="clearLogs">清空</button></div>
                <div v-if="logLines.length" class="log-list"><div v-for="line in [...logLines].slice(-5).reverse()" :key="line.id" class="log-row"><span :class="['log-mark', `log-mark--${line.level}`]"></span><time>{{ line.time }}</time><span>{{ line.message }}</span></div></div>
                <div v-else class="log-empty"><Terminal :size="19" /><span>还没有运行记录，开始后会显示在这里。</span></div>
                <button class="all-logs" type="button" @click="page = 'logs'">查看运行记录 <span>→</span></button>
              </section>

              <section class="activity-card recent-card">
                <div class="aside-heading recent-heading"><div><span class="section-kicker">LOCAL OUTPUT</span><h2><History :size="16" />最近文件</h2></div><button class="text-action" type="button" @click="page = 'files'">全部文件 <span>→</span></button></div>
                <button v-for="file in files.slice(0, 2)" :key="file.id" class="recent-row" type="button" @click="openPreview(file)"><span :class="['mini-file-icon', `file-${file.type.toLowerCase()}`]"><component :is="file.type === 'CSV' || file.type === 'XLSX' ? FileSpreadsheet : FileJson" :size="16" /></span><span class="recent-meta"><b>{{ file.name }}</b><small>{{ file.records ?? '—' }} 条 · {{ file.updated }}</small></span><span class="recent-size">{{ formatSize(file.size) }}</span></button>
              </section>
            </aside>
          </div>

          <footer class="page-footer"><span>快手采集工作台 · 本地工作空间</span><span><ShieldCheck :size="14" />请遵守平台规则，仅整理你有权访问的公开内容。</span></footer>
        </template>

        <template v-else-if="page === 'files'">
          <section class="page-intro files-intro"><div><div class="eyebrow"><span></span> LOCAL FILES <b>/</b> KUAISHOU</div><h1>数据文件</h1><p>采集结果留存在本地，可以预览内容或导出文件。</p></div><div class="file-summary"><span class="summary-icon"><Database :size="18" /></span><span><b>{{ files.length }} 个文件</b><small>{{ recordCount }} 条内容 · {{ formatSize(storageSize) }}</small></span></div></section>
          <section class="files-card">
            <div class="files-heading"><div><h2>快手采集结果</h2><p>共 {{ visibleFiles.length }} 个文件 <span>·</span> {{ settings.outputDirectory || '尚未设置输出目录' }}</p></div><div class="file-controls"><label class="file-search"><Search :size="16" /><input v-model="searchFiles" type="search" placeholder="搜索文件名" /></label><label class="file-filter"><select v-model="fileTypeFilter"><option>全部格式</option><option>JSONL</option><option>JSON</option><option>CSV</option><option>XLSX</option><option>XLS</option></select><ChevronDown :size="14" /></label><button class="quiet-button file-refresh" type="button" :disabled="filesLoading" @click="refreshFiles">{{ filesLoading ? '读取中' : '刷新' }}</button></div></div>
            <div class="table-wrap"><table class="files-table"><thead><tr><th>文件名称</th><th>采集方式</th><th>记录数</th><th>文件大小</th><th>更新时间</th><th></th></tr></thead><tbody>
              <tr v-for="file in visibleFiles" :key="file.id" @click="openPreview(file)"><td><div class="file-name"><span :class="['mini-file-icon', `file-${file.type.toLowerCase()}`]"><component :is="file.type === 'CSV' || file.type === 'XLSX' || file.type === 'XLS' ? FileSpreadsheet : FileJson" :size="17" /></span><span><b>{{ file.name }}</b><small>{{ file.type }} · 快手</small></span></div></td><td><span class="mode-tag">{{ file.mode }}</span></td><td>{{ file.records ?? '—' }} <span class="cell-muted">条</span></td><td>{{ formatSize(file.size) }}</td><td class="cell-muted">{{ file.updated }}</td><td><div class="row-actions"><button type="button" @click.stop="openPreview(file)">预览</button><button class="download-action" type="button" aria-label="导出文件" @click.stop="exportFile(file)"><Download :size="16" /></button></div></td></tr>
              <tr v-if="!visibleFiles.length"><td colspan="6"><div class="table-empty"><FolderOpen :size="23" /><b>{{ files.length ? '没有找到匹配的文件' : '这个目录还没有采集文件' }}</b><span>{{ files.length ? '换个名称或清除格式筛选试试。' : `完成一次任务后，文件会出现在 ${settings.outputDirectory}` }}</span></div></td></tr>
            </tbody></table></div>
            <div class="table-footer"><span>文件仅从本地输出目录读取</span><span>{{ settings.outputDirectory }}</span></div>
          </section>
          <div v-if="!engineStatus.pythonInstalled" class="privacy-note"><ShieldCheck :size="17" /><div><b>先安装本机运行环境</b><p>安装后这里会读取真实采集文件，并支持预览和导出。</p></div><button class="text-action" type="button" @click="page = 'settings'">前往设置</button></div>
          <div v-else class="privacy-note"><ShieldCheck :size="17" /><div><b>本地数据空间</b><p>文件只在本机读取；使用“导出”可复制原始文件到其他位置。</p></div></div>
        </template>

        <template v-else-if="page === 'logs'">
          <section class="page-intro logs-intro">
            <div><div class="eyebrow"><span></span> SESSION ACTIVITY <b>/</b> LOCAL</div><h1>运行日志</h1><p>查看采集引擎和本机运行环境输出的实时记录。</p></div>
            <button class="quiet-button" type="button" @click="clearLogs"><Trash2 :size="15" />清空日志</button>
          </section>

          <section class="logs-card" aria-label="运行日志记录">
            <div class="logs-toolbar">
              <label class="file-search logs-search" for="log-search"><Search :size="16" /><input id="log-search" v-model="logSearch" type="search" placeholder="搜索日志内容或时间" /></label>
              <label class="file-filter logs-filter" for="log-level"><span>级别</span><select id="log-level" v-model="logLevelFilter"><option value="all">全部级别</option><option value="success">成功</option><option value="info">信息</option><option value="warning">提醒</option><option value="error">错误</option><option value="debug">调试</option></select><ChevronDown :size="14" /></label>
            </div>
            <div class="logs-summary"><span>当前会话 <b>{{ logLines.length }}</b> 条记录</span><span>最多保留最近 30 条</span></div>
            <div v-if="visibleLogs.length" class="logs-list">
              <div class="logs-list-head"><span>级别</span><span>时间</span><span>记录内容</span></div>
              <div v-for="line in visibleLogs" :key="line.id" class="logs-entry">
                <span :class="['log-level', `log-level--${line.level}`]"><i></i>{{ line.level === 'success' ? '成功' : line.level === 'warning' ? '提醒' : line.level === 'error' ? '错误' : line.level === 'debug' ? '调试' : '信息' }}</span>
                <time>{{ line.time }}</time><span class="logs-message">{{ line.message }}</span>
              </div>
            </div>
            <div v-else class="logs-empty" aria-live="polite">
              <Terminal :size="22" />
              <b>{{ logLines.length ? '没有找到匹配的日志' : '当前会话还没有日志' }}</b>
              <span>{{ logLines.length ? '试试调整关键词或级别筛选。' : '启动采集任务或安装运行环境后，记录会显示在这里。' }}</span>
              <button v-if="logSearch || logLevelFilter !== 'all'" class="text-action" type="button" @click="logSearch = ''; logLevelFilter = 'all'">清除筛选</button>
            </div>
          </section>
          <div class="privacy-note"><ShieldCheck :size="17" /><div><b>本机会话日志</b><p>日志实时显示当前任务输出；清空只影响本页记录，不会删除采集文件。</p></div></div>
        </template>

        <template v-else>
          <section class="page-intro settings-intro">
            <div><div class="eyebrow"><span></span> PREFERENCES <b>/</b> LOCAL</div><h1>设置</h1><p>管理新任务默认使用的保存格式和本地输出目录偏好。</p></div>
            <span class="settings-saved"><Check :size="14" />自动保存在本机</span>
          </section>

          <div class="settings-grid">
            <section class="settings-card settings-card--engine">
              <header class="settings-card-heading"><span class="settings-icon"><Activity :size="18" /></span><div><h2>本机采集环境</h2><p>运行环境随项目单独安装，不使用系统 Python。</p></div><span :class="['engine-chip', { 'engine-chip--ready': engineStatus.ready }]"><i></i>{{ isSetupRunning ? '正在安装' : engineStatus.ready ? '已就绪' : '待安装' }}</span></header>
              <div class="engine-state-row"><span :class="{ ready: engineStatus.engineInstalled }"><Check :size="14" />采集引擎源码</span><span :class="{ ready: engineStatus.pythonInstalled }"><Check :size="14" />Python 3.11</span><span :class="{ ready: engineStatus.browserInstalled }"><Check :size="14" />Chromium 浏览器</span></div>
              <div class="engine-action-row"><p>{{ engineStatus.message }}。安装过程会在“运行日志”页显示。</p><button v-if="!engineStatus.ready" class="run-button" type="button" :disabled="isSetupRunning || isRunning" @click="setupBackend"><Download :size="15" />{{ isSetupRunning ? '正在准备…' : '安装运行环境' }}</button><button v-else class="quiet-button" type="button" @click="loadEngineStatus">重新检查</button></div>
            </section>

            <section class="settings-card">
              <header class="settings-card-heading"><span class="settings-icon"><FileJson :size="18" /></span><div><h2>默认保存格式</h2><p>新建任务时会自动选用此格式。</p></div></header>
              <div class="settings-control-row">
                <div><label for="default-format">文件格式</label><small>任务创建后，仍可在“更多采集选项”中单独修改。</small></div>
                <label class="settings-select" for="default-format"><component :is="settings.defaultFormat === 'csv' || settings.defaultFormat === 'excel' ? FileSpreadsheet : FileJson" :size="16" /><select id="default-format" v-model="settings.defaultFormat"><option value="jsonl">JSONL 文件</option><option value="json">JSON 文件</option><option value="csv">CSV 表格</option><option value="excel">Excel 表格</option></select><ChevronDown :size="14" /></label>
              </div>
              <div class="settings-hint"><span class="hint-dot"></span>当前默认格式：<b>{{ formatName(settings.defaultFormat) }}</b></div>
            </section>

            <section class="settings-card">
              <header class="settings-card-heading"><span class="settings-icon settings-icon--folder"><FolderOpen :size="18" /></span><div><h2>输出目录偏好</h2><p>保存你希望使用的本地目录路径。</p></div></header>
              <div class="settings-control-row settings-control-row--stacked">
                <div><label for="output-directory">目录路径</label><small>相对路径保存在应用数据目录下，也可以选择其他本机文件夹。</small></div>
                <div class="directory-picker-row"><label class="settings-path" for="output-directory"><FolderOpen :size="16" /><input id="output-directory" v-model="settings.outputDirectory" type="text" placeholder="例如：data/ks/" autocomplete="off" spellcheck="false" /></label><button class="quiet-button" type="button" @click="selectOutputDirectory">浏览</button></div>
              </div>
              <div class="settings-hint"><span class="hint-dot"></span>相对路径以应用本地数据目录为基准，新任务会直接写入。</div>
            </section>
          </div>

          <div class="privacy-note settings-privacy"><ShieldCheck :size="17" /><div><b>本机偏好设置</b><p>设置保存在本机；输出目录只用于创建任务和读取真实采集文件。</p></div></div>
        </template>
      </div>
    </main>

    <Transition name="toast"><div v-if="toast" class="toast-message"><Check :size="16" />{{ toast }}</div></Transition>
    <Transition name="drawer"><div v-if="selectedFile" class="drawer-backdrop" @click.self="closePreview"><aside class="preview-drawer" aria-label="文件预览">
      <header class="drawer-heading"><div><span class="section-kicker">LOCAL PREVIEW</span><h2>采集内容</h2></div><button class="close-button" type="button" aria-label="关闭预览" @click="closePreview"><X :size="18" /></button></header>
      <div class="drawer-file"><span :class="['mini-file-icon', `file-${selectedFile.type.toLowerCase()}`]"><component :is="selectedFile.type === 'CSV' || selectedFile.type === 'XLSX' || selectedFile.type === 'XLS' ? FileSpreadsheet : FileJson" :size="17" /></span><span><b>{{ selectedFile.name }}</b><small>{{ selectedFile.records ?? '—' }} 条记录 · {{ formatSize(selectedFile.size) }}</small></span></div>
      <div v-if="previewLoading" class="preview-loading"><Activity :size="19" />正在读取文件内容…</div>
      <div v-else-if="selectedFile.preview.length" class="preview-table-wrap"><table class="preview-table"><thead><tr><th v-for="key in selectedFile.columns" :key="key">{{ key }}</th></tr></thead><tbody><tr v-for="(row, index) in selectedFile.preview" :key="index"><td v-for="key in selectedFile.columns" :key="key">{{ formatCell(row[key]) }}</td></tr></tbody></table></div>
      <div v-else class="preview-loading">文件没有可预览的记录。</div>
      <div class="preview-count"><span>预览 {{ selectedFile.preview.length }} 条 / 共 {{ selectedFile.records ?? '未知' }} 条</span><span>读取本机文件</span></div>
      <footer class="drawer-actions"><button class="quiet-button" type="button" @click="closePreview">关闭</button><button class="run-button" type="button" @click="downloadSelected"><ArrowDownToLine :size="15" />导出文件</button></footer>
    </aside></div></Transition>
  </div>
</template>
