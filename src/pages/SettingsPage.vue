<script setup>
import Icon from "../components/Icon.vue";
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from "vue";
import { animate, stagger } from "animejs";
import * as api from "../api";
import { IMAGE_CHOICES, VIDEO_QUALITIES } from "../download-request.js";

const props = defineProps({
  login: { type: Object, required: true },
  /** 已保存的设置（后端结构） */
  settings: { type: Object, default: null },
  env: { type: Object, default: null },
});
const emit = defineEmits(["toast", "save", "reset", "reload", "login", "logout"]);

// 分类图标：值是 iconfont 图标名（见 src/icons.js）
const CATEGORY_ICONS = {
  download: "download",
  media: "album",
  naming: "fileText",
  folder: "folder",
  update: "refresh",
  network: "server",
};

const categories = [
  { key: "download", label: "下载", hint: "目录、并发与恢复" },
  { key: "media", label: "媒体", hint: "图片、视频与文案" },
  { key: "naming", label: "文件命名", hint: "模板与重名处理" },
  { key: "folder", label: "文件夹", hint: "层级与文件夹命名" },
  { key: "update", label: "应用更新", hint: "版本检测与安装" },
  { key: "network", label: "网络与维护", hint: "代理、日志与数据" },
];

const active = ref("download");

// 分类切换编排：右侧字段行依次浮起（v-if 内切换不走 App 的切页 watch）。
// 结束后清掉内联 transform：恒等矩阵也会创建层叠上下文，
// 把字段里的魔法变量面板压到后续字段之下（实测踩过）。
watch(active, async () => {
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  await nextTick();
  requestAnimationFrame(() => {
    const fields = document.querySelector(".card .fields");
    if (!fields) return;
    const kids = [...fields.children].slice(0, 14);
    if (kids.length) {
      animate(kids, { opacity: [0, 1], translateY: [8, 0], duration: 380, delay: stagger(32), ease: "outExpo" }).then(
        () => kids.forEach((k) => (k.style.transform = "")),
      );
    }
  });
});
const activeCategory = computed(() =>
  categories.find((category) => category.key === active.value)
);

/** 本地草稿：编辑期间不落盘，点「保存」才提交 */
const draft = ref(null);

// 命名预设只决定"条目自己叫什么"，**不写目录层级**：层级由「文件夹」页的规则负责。
// 所以这里一律不含 `/`，也不重复使用文件夹模板里的变量。
// 每个预设对应**一种来源形状**：单条／批量列表／带作者／按日期。
const BUILTIN_PRESETS = [
  { name: "批量带序号（默认）", template: "{index} {title}.{ext}" },
  { name: "单文件", template: "{title}.{ext}" },
  { name: "带作者名", template: "{author} - {title}.{ext}" },
  { name: "按发布日期", template: "{publish_date} {title}.{ext}" },
];

/** 内置名不许被用户预设顶掉：重名直接拒绝保存 */
function builtinNameTaken(name, builtins) {
  return builtins.some((preset) => preset.name === name);
}

/** 预设名由模板反推：改了模板下拉就跟着变。自己的预设排在前面——
    刚存完要能在下拉里看见自己起的名字，哪怕模板和内置信一模一样。 */
const selectedPreset = computed(() => {
  const template = draft.value?.naming_template ?? "";
  const saved = (draft.value?.naming_presets ?? []).find((preset) => preset.template === template);
  if (saved) return saved.name;
  const builtin = BUILTIN_PRESETS.find((preset) => preset.template === template);
  return builtin ? builtin.name : "";
});
const presetName = ref("");
const folderPresetName = ref("");

const proxyDraft = ref("");
const dataDirDraft = ref("");
const cleaning = ref("");

// 应用更新：优先走 updater 插件（带签名校验，可直接下载安装）；
// 插件不可用时回退后端只读检测（GitHub Releases 最新 tag）
const checkingUpdate = ref(false);
const updateResult = ref(null);
const pluginUpdate = ref(null);
const downloading = ref(false);
const downloadPct = ref(0);
const installed = ref(false);

async function checkUpdate() {
  if (checkingUpdate.value) return;
  checkingUpdate.value = true;
  installed.value = false;
  downloadPct.value = 0;
  try {
    const update = await api.updateCheckPlugin().catch(() => null);
    if (update) {
      // 插件发现了新版本（带签名的更新包，可直接下载安装）
      pluginUpdate.value = update;
      updateResult.value = {
        current: update.currentVersion,
        latest: update.version,
        up_to_date: false,
        error: "",
      };
      emit("toast", `发现新版本 v${update.version}`);
    } else {
      // 插件返回 null = 已是最新（签名校验过的权威结论），不再绕后端兜底——
      // 之前这里会去请求 GitHub API 兜底，网络不好时反而显示"无法检测"，误导
      pluginUpdate.value = null;
      updateResult.value = {
        current: props.settings?.version || "",
        latest: "",
        up_to_date: true,
        error: "",
      };
      emit("toast", `已是最新版本（v${props.settings?.version || ""}）`);
    }
  } catch (error) {
    updateResult.value = {
      current: props.settings?.version || "",
      latest: "",
      up_to_date: false,
      error: String(error),
    };
    emit("toast", `检测更新失败：${error}`);
  } finally {
    checkingUpdate.value = false;
  }
}

/** 下载并安装更新（签名校验 + 静默安装；完成后重启生效） */
async function installUpdate() {
  if (downloading.value || !pluginUpdate.value) return;
  downloading.value = true;
  downloadPct.value = 0;
  try {
    await api.updateDownloadAndInstall(pluginUpdate.value, ({ received, total }) => {
      downloadPct.value = total ? Math.round((received / total) * 100) : 0;
    });
    installed.value = true;
    emit("toast", "更新已安装，点「立即重启」生效");
  } catch (error) {
    emit("toast", `更新安装失败：${error}`);
  } finally {
    downloading.value = false;
  }
}

const dirty = computed(() => {
  if (!props.settings || !draft.value) return false;
  return JSON.stringify(draft.value) !== JSON.stringify(props.settings);
});

function beginDraft() {
  draft.value = props.settings ? { ...props.settings } : null;
}
beginDraft();

// 媒体页的两个单值下拉都来自共享定义（选项说明写在标签里，下拉栏内一眼看清；
// 与引导页/下载弹层同一份，见 download-request.js）

const AUDIO_QUALITIES = [
  { value: "best", label: "最佳音质" },
  { value: "standard", label: "标准音质" },
];

const RENAME_CONFLICTS = [
  { value: "skip", label: "已有文件则跳过（推荐）" },
  { value: "overwrite", label: "覆盖已有文件" },
  { value: "auto", label: "自动重命名（追加序号）" },
];

const LOG_LEVELS = [
  { value: "debug", label: "调试" },
  { value: "info", label: "信息" },
  { value: "warn", label: "警告" },
  { value: "error", label: "错误" },
];

const PARSE_PRESETS = [
  { value: "标准", batch: 8, wait: 1000, every: 100, rest: 3000 },
  { value: "快速", batch: 16, wait: 500, every: 200, rest: 2000 },
  { value: "谨慎", batch: 3, wait: 2000, every: 50, rest: 5000 },
];

const CONCURRENCY = [1, 2, 3, 4, 5];
const RETRIES = [0, 1, 2, 3, 5, 8];
const SPEEDS = [0, 1, 2, 5, 10, 20, 50];

const proxyValue = computed({
  get: () => (proxyDraft.value !== "" ? proxyDraft.value : draft.value?.proxy ?? ""),
  set: (value) => (proxyDraft.value = value),
});
const dataDirValue = computed({
  get: () => (dataDirDraft.value !== "" ? dataDirDraft.value : draft.value?.data_dir ?? ""),
  set: (value) => (dataDirDraft.value = value),
});

/** 本地时区的 YYYY-MM-DD；不带参数即今天 */
function localDate(unixSecs) {
  const d = unixSecs === undefined ? new Date() : new Date(unixSecs * 1000);
  const pad = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

// 预览走后端同一个渲染器：预览里能出什么，落盘就能出什么
const namingPreview = ref("");
// 文件夹层级：输入框引用 + 预览 + 弹出面板状态
const folderInput = ref(null);
const folderPreview = ref("");
const pickingFolderVar = ref(false);
const folderPicker = ref(null);

/** 两套模板的变量重合提醒：同一个变量在文件名与文件夹里都出现 = 信息重复 */
const overlapVars = computed(() => {
  const pick = (tpl) => [...String(tpl ?? "").matchAll(/\{(\w+)\}/g)].map((m) => m[1]);
  const folder = new Set(pick(draft.value?.folder_template));
  const duplicated = [...new Set(pick(draft.value?.naming_template))]
    .filter((v) => folder.has(v))
    .map((v) => `{${v}}`);
  return { folderOnly: [...folder], duplicated };
});

const FOLDER_PRESETS = [
  // 第一条 = 后端的默认层级：软件没设置过文件夹模板时用它
  { name: "作者 → 年月（默认）", template: "{author}/{year}-{month}" },
  { name: "只按作者分层", template: "{author}" },
  { name: "作者 → 来源类型", template: "{author}/{source_kind}" },
  { name: "不建文件夹（全部平铺）", template: "" },
];
let previewSeq = 0;

async function refreshPreview() {
  const template = draft.value?.naming_template ?? "";
  const seq = ++previewSeq;
  try {
    const text = await api.previewNaming(template, {
      date: localDate(),
      publish_date: "2025-10-01",
      ext: "jpg",
    });
    // 连续敲键会有多次请求，只认最后一次的结果
    if (seq === previewSeq) namingPreview.value = text;
  } catch {
    if (seq === previewSeq) namingPreview.value = "";
  }

  const folder = draft.value?.folder_template ?? "";
  const fseq = ++previewSeq;
  try {
    const text = await api.previewNaming(folder, { date: localDate(), dir: true });
    if (fseq === previewSeq) folderPreview.value = text;
  } catch {
    if (fseq === previewSeq) folderPreview.value = "";
  }
}

watch(
  () => [draft.value?.naming_template, draft.value?.folder_template],
  refreshPreview,
  { immediate: true }
);

// 变量清单与插入面板
const variables = ref([]);
const pickingVar = ref(false);
const varPicker = ref(null);
const templateInput = ref(null);

/** 面板里的栏目：一列一组，横排。
    顺序就是 Rust 那张表的顺序，同名栏目保证连续，遇到新名字就新开一列即可。 */
const variableColumns = computed(() => {
  const out = [];
  for (const item of variables.value) {
    const name = item.section || "其他";
    if (out[out.length - 1]?.name !== name) out.push({ name, items: [] });
    out[out.length - 1].items.push(item);
  }
  return out;
});

async function loadVariables() {
  try {
    const list = await api.namingVariables();
    // 显示文本在这里拼好：模板里直接写 `{...}` 会和 Vue 的插值定界符打架
    variables.value = list.map((item) => ({ ...item, text: `{${item.token}}` }));
  } catch (error) {
    // 不静默吞：面板空掉时必须能在控制台看到真实原因
    console.error("魔法变量加载失败:", error);
    variables.value = [];
  }
}

/** 插到光标处；没有焦点时追加到末尾，插完把光标放到标记之后。
 *  target 决定写进哪个字段：文件名模板 or 文件夹层级模板。 */
function insertToken(token, target = "naming") {
  if (!draft.value) return;
  const snippet = `{${token}}`;
  const key = target === "folder" ? "folder_template" : "naming_template";
  const el = target === "folder" ? folderInput.value : templateInput.value;
  if (!el) {
    draft.value[key] = `${draft.value[key] ?? ""}${snippet}`;
    return;
  }
  const start = el.selectionStart ?? el.value.length;
  const end = el.selectionEnd ?? start;
  draft.value[key] = el.value.slice(0, start) + snippet + el.value.slice(end);
  nextTick(() => {
    el.focus();
    const caret = start + snippet.length;
    el.setSelectionRange(caret, caret);
  });
}

function onVarDocumentDown(event) {
  if (pickingVar.value && varPicker.value && !varPicker.value.contains(event.target)) {
    pickingVar.value = false;
  }
  if (pickingFolderVar.value && folderPicker.value && !folderPicker.value.contains(event.target)) {
    pickingFolderVar.value = false;
  }
}

function onVarKeydown(event) {
  if (event.key === "Escape") {
    pickingVar.value = false;
    pickingFolderVar.value = false;
  }
}

onMounted(() => {
  loadVariables();
  document.addEventListener("mousedown", onVarDocumentDown);
  document.addEventListener("keydown", onVarKeydown);
});

onUnmounted(() => {
  document.removeEventListener("mousedown", onVarDocumentDown);
  document.removeEventListener("keydown", onVarKeydown);
});

const parsePaceNote = computed(() => {
  const batch = draft.value?.parse_batch ?? 8;
  const wait = draft.value?.parse_batch_wait_ms ?? 1000;
  const every = draft.value?.parse_rest_every ?? 100;
  const rest = draft.value?.parse_rest_ms ?? 3000;
  const total = 200;
  const batches = Math.ceil(total / batch);
  const extraMs = Math.max(batches - 1, 0) * wait + Math.floor(total / every) * rest;
  return `解析约 ${total} 条来源时，额外等待约 ${Math.round(extraMs / 1000)} 秒（不含网络耗时）。`;
});

function set(key, value) {
  if (draft.value) draft.value[key] = value;
}

/** 选预设：内置或用户保存的，选中即填入模板（下拉显示什么由模板决定） */
function selectPreset(name) {
  if (!name || !draft.value) return;
  const builtin = BUILTIN_PRESETS.find((preset) => preset.name === name);
  if (builtin) {
    draft.value.naming_template = builtin.template;
    return;
  }
  const saved = draft.value.naming_presets?.find((preset) => preset.name === name);
  if (saved) draft.value.naming_template = saved.template;
}

/** 文件夹页的预设名同样由模板反推，「自定义模板」就是没有预设对得上的时候 */
const selectedFolderPreset = computed(() => {
  const template = draft.value?.folder_template ?? "";
  const saved = (draft.value?.folder_presets ?? []).find((preset) => preset.template === template);
  if (saved) return saved.name;
  const builtin = FOLDER_PRESETS.find((preset) => preset.template === template);
  return builtin ? builtin.name : "";
});

function selectFolderPreset(name) {
  if (!name || !draft.value) return;
  const builtin = FOLDER_PRESETS.find((preset) => preset.name === name);
  if (builtin) {
    draft.value.folder_template = builtin.template;
    return;
  }
  const saved = draft.value.folder_presets?.find((preset) => preset.name === name);
  if (saved) draft.value.folder_template = saved.template;
}

/** 保存文件夹预设：模板可以是空的（不建文件夹） */
function saveFolderPreset() {
  const name = folderPresetName.value.trim();
  if (!name || !draft.value) {
    emit("toast", "先填写预设名称，再保存为预设");
    return;
  }
  if (builtinNameTaken(name, FOLDER_PRESETS)) {
    emit("toast", `「${name}」和内置层级重名了，换一个名字（内置清单不会被覆盖）`);
    return;
  }
  const presets = [...(draft.value.folder_presets ?? [])];
  const existing = presets.findIndex((preset) => preset.name === name);
  if (existing >= 0) {
    presets[existing] = { name, template: draft.value.folder_template };
  } else {
    presets.push({ name, template: draft.value.folder_template });
  }
  draft.value.folder_presets = presets;
  folderPresetName.value = "";
  emit("toast", `预设「${name}」已加入，点上方「保存」生效`);
}

/** 保存为预设：同名更新模板，随设置一起落盘 */
function savePreset() {
  const name = presetName.value.trim();
  if (!name || !draft.value) {
    emit("toast", "先填写预设名称，再保存为预设");
    return;
  }
  if (builtinNameTaken(name, BUILTIN_PRESETS)) {
    emit("toast", `「${name}」和内置预设重名了，换一个名字（内置清单不会被覆盖）`);
    return;
  }
  const presets = [...(draft.value.naming_presets ?? [])];
  const existing = presets.findIndex((preset) => preset.name === name);
  if (existing >= 0) {
    presets[existing] = { name, template: draft.value.naming_template };
  } else {
    presets.push({ name, template: draft.value.naming_template });
  }
  draft.value.naming_presets = presets;
  presetName.value = "";
  emit("toast", `预设「${name}」已加入，点上方「保存」生效`);
}

function applyPreset(name) {
  const preset = PARSE_PRESETS.find((item) => item.value === name);
  if (!preset || !draft.value) return;
  draft.value.parse_preset = preset.value;
  draft.value.parse_batch = preset.batch;
  draft.value.parse_batch_wait_ms = preset.wait;
  draft.value.parse_rest_every = preset.every;
  draft.value.parse_rest_ms = preset.rest;
}

async function chooseDir() {
  const dir = await api.chooseOutputDir().catch(() => "");
  if (dir) set("output_dir", dir);
}

async function chooseDataDir() {
  const dir = await api.chooseOutputDir().catch(() => "");
  if (dir) {
    dataDirDraft.value = "";
    set("data_dir", dir);
  }
}

function commitProxy() {
  set("proxy", proxyValue.value.trim());
  proxyDraft.value = "";
}

const savedFlash = ref(false);
let savedTimer = 0;
function save() {
  if (!dirty.value) return;
  emit("save", { ...draft.value });
  // 收官动效：保存按钮成功脉冲（toast 之外按钮本体的确认反馈）
  if (!matchMedia("(prefers-reduced-motion: reduce)").matches) {
    savedFlash.value = false;
    requestAnimationFrame(() => {
      savedFlash.value = true;
      clearTimeout(savedTimer);
      savedTimer = setTimeout(() => (savedFlash.value = false), 450);
    });
  }
}

function undo() {
  beginDraft();
  proxyDraft.value = "";
  dataDirDraft.value = "";
  emit("reload");
}

function reset() {
  emit("reset");
  beginDraft();
}

async function cleanup(kind) {
  cleaning.value = kind;
  try {
    if (kind === "temp") {
      const count = await api.cleanupTemp();
      emit("toast", count ? `已清理 ${count} 项临时文件` : "没有可清理的临时文件");
    } else if (kind === "diag") {
      const path = await api.exportDiagnostics();
      emit("toast", path ? `诊断信息已导出：${path}` : "已取消导出");
    }
  } catch (error) {
    emit("toast", String(error));
  } finally {
    cleaning.value = "";
  }
}
</script>

<template>
  <div>
    <section class="card">
      <header class="head">
        <h1>设置</h1>
        <span class="spacer"></span>
        <button class="ghost" :disabled="!settings" @click="reset">
          <Icon name="reload" />
          恢复默认
        </button>
        <button class="ghost" :disabled="!dirty" @click="undo">
          <Icon name="undo" />
          撤销
        </button>
        <button class="primary" :class="{ 'saved-flash': savedFlash }" :disabled="!dirty" @click="save">
          <Icon name="check" />
          保存
        </button>
      </header>

      <div v-if="draft" class="layout">
        <nav class="cats">
          <button
            v-for="category in categories"
            :key="category.key"
            :class="{ active: active === category.key }"
            @click="active = category.key"
          >
            <Icon :name="CATEGORY_ICONS[category.key]" class="icon" />
            <span class="text">
              <span class="label">{{ category.label }}</span>
              <span class="hint">{{ category.hint }}</span>
            </span>
          </button>
        </nav>

        <div class="panel">
          <div class="panel-head">
            <Icon :name="CATEGORY_ICONS[activeCategory.key]" class="panel-icon" />
            <div>
              <h2>{{ activeCategory.label }}</h2>
              <p class="panel-hint">{{ activeCategory.hint }}</p>
            </div>
          </div>

          <!-- 下载 -->
          <div v-if="active === 'download'" class="fields page-in">
            <div class="field full">
              <label>保存目录</label>
              <div class="row-flex">
                <input v-model="draft.output_dir" spellcheck="false" />
                <button class="ghost" @click="chooseDir">选择</button>
              </div>
            </div>

            <div class="grid2">
              <div class="field">
                <label>同时下载任务数</label>
                <select v-model.number="draft.max_concurrent_tasks">
                  <option v-for="n in CONCURRENCY" :key="n" :value="n">{{ n }}</option>
                </select>
              </div>
              <div class="field">
                <label>失败自动重试次数</label>
                <select v-model.number="draft.retry_count">
                  <option v-for="n in RETRIES" :key="n" :value="n">{{ n }}</option>
                </select>
              </div>
            </div>

            <div class="field full">
              <label>全局下载限速（MiB/s）</label>
              <select v-model.number="draft.speed_limit_mib">
                <option v-for="n in SPEEDS" :key="n" :value="n">
                  {{ n === 0 ? "不限速" : `${n} MiB/s` }}
                </option>
              </select>
            </div>

            <div class="checks full">
              <label class="check">
                <input type="checkbox" v-model="draft.resume_on_start" />
                <span>启动时自动继续未完成任务</span>
              </label>
              <label class="check">
                <input type="checkbox" v-model="draft.keep_temp" />
                <span>保留临时文件（调试用）</span>
              </label>
            </div>

            <div class="group full">
              <div class="group-title">
                解析节奏
                <span class="info" title="批量解析时按此节奏分批请求，降低触发风控的概率">?</span>
              </div>
              <div class="grid2">
                <div class="field">
                  <label>解析预设</label>
                  <select
                    :value="draft.parse_preset"
                    @change="applyPreset($event.target.value)"
                  >
                    <option v-for="preset in PARSE_PRESETS" :key="preset.value" :value="preset.value">
                      {{ preset.value }}
                    </option>
                  </select>
                </div>
                <div class="field">
                  <label>每批解析 <span class="info" title="一轮里同时解析几条">?</span></label>
                  <select v-model.number="draft.parse_batch">
                    <option :value="3">3 条</option>
                    <option :value="8">8 条</option>
                    <option :value="16">16 条</option>
                    <option :value="30">30 条</option>
                  </select>
                </div>
                <div class="field">
                  <label>批间等待 <span class="info" title="两轮解析之间等多久，给接口留出间隔">?</span></label>
                  <select v-model.number="draft.parse_batch_wait_ms">
                    <option :value="500">0.5 秒</option>
                    <option :value="1000">1 秒</option>
                    <option :value="2000">2 秒</option>
                    <option :value="3000">3 秒</option>
                  </select>
                </div>
                <div class="field">
                  <label>每 {{ draft.parse_rest_every }} 条休息 <span class="info" title="累计解析这么多条后额外休息一次">?</span></label>
                  <div class="row-flex">
                    <select v-model.number="draft.parse_rest_every">
                      <option :value="50">每 50 条</option>
                      <option :value="100">每 100 条</option>
                      <option :value="200">每 200 条</option>
                    </select>
                    <select v-model.number="draft.parse_rest_ms">
                      <option :value="2000">2 秒</option>
                      <option :value="3000">3 秒</option>
                      <option :value="5000">5 秒</option>
                    </select>
                  </div>
                </div>
              </div>
              <p class="note">{{ parsePaceNote }}</p>
            </div>

            <div class="env full">
              <div class="env-head">
                <span class="env-title">登录状态 <i>Xiaohongshu</i></span>
                <span class="spacer"></span>
                <span class="badge" :class="login.logged_in ? 'ok' : 'bad'">
                  {{ login.logged_in ? "已登录" : "未登录" }}
                </span>
              </div>
              <div class="env-row" :class="{ bad: !login.logged_in }">
                <Icon name="user" class="env-icon" />
                <div class="env-text">
                  <span class="env-name">{{ login.logged_in ? login.uname : "微信账号" }}</span>
                  <span class="env-info">
                    {{ login.logged_in ? `ID ${login.mid}` : "公众平台登录后可枚举任意公众号全部文章（下一版本提供）" }}
                  </span>
                </div>
                <button
                  class="env-recheck"
                  @click="login.logged_in ? emit('logout') : emit('login')"
                >
                  {{ login.logged_in ? "退出登录" : "登录账号" }}
                </button>
              </div>
            </div>
          </div>

          <!-- 媒体 -->
          <div v-else-if="active === 'media'" class="fields page-in">
            <div class="grid2">
              <div class="field">
                <label>图片规格</label>
                <select v-model="draft.image_format">
                  <option v-for="item in IMAGE_CHOICES" :key="item.value" :value="item.value">
                    {{ item.label }}
                  </option>
                </select>
              </div>
              <div class="field">
                <label>视频清晰度</label>
                <select v-model="draft.video_quality">
                  <option v-for="item in VIDEO_QUALITIES" :key="item.value" :value="item.value">
                    {{ item.label }}
                  </option>
                </select>
              </div>
            </div>
            <p class="note">
              公众号图片按微信放出的最高画质保存；文章内视频自动解析最高画质直链
              （网页端常见 1080P / 720P 档）。
            </p>

            <label class="check card-check full">
              <input type="checkbox" v-model="draft.download_text" />
              <span>下载文案（独立 .txt，与媒体文件放在一起）</span>
            </label>
            <p v-if="draft.download_text" class="note">
              文案存成<b>与媒体同名的 .txt</b>（取标题或正文前 30 字做文件名），
              包含正文、发布时间与原链接，不含评论。
            </p>
          </div>

          <!-- 文件命名 -->
          <div v-else-if="active === 'naming'" class="fields page-in">
            <div class="field full">
              <label>命名预设</label>
              <select :value="selectedPreset" @change="selectPreset($event.target.value)">
                <option value="">自定义模板</option>
                <optgroup label="内置">
                  <option v-for="preset in BUILTIN_PRESETS" :key="preset.name" :value="preset.name">
                    {{ preset.name }}
                  </option>
                </optgroup>
                <optgroup v-if="draft.naming_presets?.length" label="我的预设">
                  <option
                    v-for="preset in draft.naming_presets"
                    :key="preset.name"
                    :value="preset.name"
                  >
                    {{ preset.name }}
                  </option>
                </optgroup>
              </select>
            </div>

            <div class="field full" :class="{ lift: pickingVar }">
              <label>命名模板</label>
              <div class="row-flex">
                <input ref="templateInput" v-model="draft.naming_template" spellcheck="false" />
                <div ref="varPicker" class="var-picker">
                  <button
                    class="ghost"
                    :class="{ on: pickingVar }"
                    title="插入变量"
                    aria-haspopup="menu"
                    :aria-expanded="pickingVar"
                    @click="pickingVar = !pickingVar"
                  >
                    <Icon name="plus" />
                  </button>

                  <Transition name="picker">
                    <div v-if="pickingVar" class="var-panel">
                      <div class="var-head">
                        <b>魔法变量</b>
                        <span>点击后插入到光标位置</span>
                      </div>
                      <div class="var-grid">
                        <section v-for="column in variableColumns" :key="column.name" class="var-col">
                          <p class="var-col-name">{{ column.name }}</p>
                          <button
                            v-for="item in column.items"
                            :key="item.token"
                            class="var-item"
                            :title="item.hint || item.label"
                            @click="insertToken(item.token)"
                          >
                            <code>{{ item.text }}</code>
                            <span>{{ item.label }}</span>
                          </button>
                        </section>
                      </div>
                    </div>
                  </Transition>
                </div>
              </div>
              <p class="note">
                <b>命名规则决定"条目自己叫什么"</b>：图片/视频/文案各自成文件，
                同一篇文章的媒体共用同一个名字前缀。
                目录层级由「文件夹」页的规则决定，最终路径 = 文件夹规则 + 命名规则。
              </p>
              <p class="note">
                文件名预览：<b>{{ namingPreview }}</b>
              </p>
              <p v-if="overlapVars.duplicated.length" class="note warn">
                这些变量在「文件夹」模板里也用了：{{ overlapVars.duplicated.join("、") }}
                —— 路径里会出现重复信息（例如作者名既在目录又在文件名）。想让层级只由文件夹模板负责，就从文件名里去掉它们。
              </p>
            </div>

            <div class="field full">
              <label>预设名称</label>
              <div class="row-flex">
                <input
                  v-model="presetName"
                  spellcheck="false"
                  placeholder="例如：收藏用命名"
                  @keydown.enter="savePreset"
                />
                <button class="ghost" @click="savePreset">
                  <Icon name="check" />
                  保存为预设
                </button>
              </div>
              <p class="note">同名预设会更新模板。预设随设置一起保存（点上方「保存」生效），下次可直接选用。</p>
            </div>

            <div class="field full">
              <label>重名处理</label>
              <select v-model="draft.rename_conflict">
                <option v-for="item in RENAME_CONFLICTS" :key="item.value" :value="item.value">
                  {{ item.label }}
                </option>
              </select>
              <p class="note">
                {{
                  draft.rename_conflict === "skip"
                    ? "最终文件已存在时跳过整个任务，未完成的分片缓存仍会继续恢复。"
                    : draft.rename_conflict === "auto"
                      ? "文件名后追加 (1) (2) … 序号，直到不冲突。"
                      : "直接覆盖已存在的同名文件。"
                }}
              </p>
            </div>
          </div>

          <!-- 文件夹层级 -->
          <div v-else-if="active === 'folder'" class="fields page-in">
            <div class="field full">
              <label>层级预设</label>
              <select :value="selectedFolderPreset" @change="selectFolderPreset($event.target.value)">
                <option value="">自定义模板</option>
                <optgroup label="内置">
                  <option v-for="preset in FOLDER_PRESETS" :key="preset.name" :value="preset.name">
                    {{ preset.name }}
                  </option>
                </optgroup>
                <optgroup v-if="draft.folder_presets?.length" label="我的预设">
                  <option
                    v-for="preset in draft.folder_presets"
                    :key="preset.name"
                    :value="preset.name"
                  >
                    {{ preset.name }}
                  </option>
                </optgroup>
              </select>
            </div>

            <div class="field full" :class="{ lift: pickingFolderVar }">
              <label>文件夹模板</label>
              <div class="row-flex">
                <input ref="folderInput" v-model="draft.folder_template" spellcheck="false" placeholder="留空表示不建文件夹" />
                <div ref="folderPicker" class="var-picker">
                  <button
                    class="ghost"
                    :class="{ on: pickingFolderVar }"
                    title="插入变量"
                    aria-haspopup="menu"
                    :aria-expanded="pickingFolderVar"
                    @click="pickingFolderVar = !pickingFolderVar"
                  >
                    <Icon name="plus" />
                  </button>

                  <Transition name="picker">
                    <div v-if="pickingFolderVar" class="var-panel">
                      <div class="var-head">
                        <b>魔法变量</b>
                        <span>点击后插入到光标位置</span>
                      </div>
                      <div class="var-grid">
                        <section v-for="column in variableColumns" :key="column.name" class="var-col">
                          <p class="var-col-name">{{ column.name }}</p>
                          <button
                            v-for="item in column.items"
                            :key="item.token"
                            class="var-item"
                            :title="item.hint || item.label"
                            @click="insertToken(item.token, 'folder')"
                          >
                            <code>{{ item.text }}</code>
                            <span>{{ item.label }}</span>
                          </button>
                        </section>
                      </div>
                    </div>
                  </Transition>
                </div>
              </div>
              <p class="note">
                文件夹预览：<b>{{ folderPreview || "（不建文件夹）" }}</b>
              </p>
            </div>

            <div class="field full">
              <label>预设名称</label>
              <div class="row-flex">
                <input
                  v-model="folderPresetName"
                  spellcheck="false"
                  placeholder="例如：按年月分目录"
                  @keydown.enter="saveFolderPreset"
                />
                <button class="ghost" @click="saveFolderPreset">
                  <Icon name="check" />
                  保存为预设
                </button>
              </div>
              <p class="note">
                同名预设会更新模板。预设随设置一起保存（点上方「保存」生效），下次可直接选用；
                模板留空也能存——"不建文件夹"就是个正当的预设。
              </p>
            </div>

            <div class="field full">
              <label>层级规则</label>
              <p class="note">
                解析用户主页时按 <b>作者 → 年月 → 条目</b> 分层：
                合集名取自合集，同一公众号的内容自动归档到一起。
                单篇文章链接也会进公众号目录，除非模板里不写作者变量。
                想区分来源，可在模板里用 <code>{source_kind}</code>（单篇文章 / 合集）。
                条目自身的文件名仍由「文件命名」决定，最终路径 = 文件夹层级 + 文件名。
              </p>
            </div>
          </div>

          <!-- 应用更新 -->
          <div v-else-if="active === 'update'" class="fields page-in">
            <div class="field full">
              <label>自动检测</label>
              <p class="note top">
                默认关闭。开启后会在应用启动时静默检测新版本，不会自动下载或安装。
              </p>
              <label class="switch">
                <input type="checkbox" v-model="draft.update_check" />
                <span class="track"><span class="knob"></span></span>
              </label>
            </div>

            <div class="field full">
              <label>当前版本</label>
              <p class="version num">v{{ env?.version || "—" }}</p>
              <button class="ghost" :disabled="checkingUpdate" @click="checkUpdate">
                {{ checkingUpdate ? "检测中…" : "检测更新" }}
              </button>
              <p v-if="updateResult" class="note top">
                <template v-if="updateResult.error">无法检测：{{ updateResult.error }}</template>
                <template v-else-if="updateResult.up_to_date">
                  已是最新版本（v{{ updateResult.current }}）
                </template>
                <template v-else>
                  发现新版本 v{{ updateResult.latest }}。
                  <button
                    v-if="!installed"
                    class="primary"
                    style="margin-left: 10px"
                    :disabled="downloading"
                    @click="installUpdate"
                  >
                    {{ downloading ? `下载并安装中 ${downloadPct}%` : "下载并安装" }}
                  </button>
                  <button
                    v-else
                    class="primary"
                    style="margin-left: 10px"
                    @click="api.relaunchApp()"
                  >
                    立即重启
                  </button>
                  <span v-if="downloading" class="num" style="margin-left: 8px">{{ downloadPct }}%</span>
                </template>
              </p>
            </div>

            <p class="note top dim">
              更新包通过 GitHub Releases 分发（仓库 yungumax/WXdown）；仓库发布新 Release 后，此处即可检测到新版本，下载安装带签名校验。
            </p>
          </div>

          <!-- 网络与维护 -->
          <div v-else class="fields page-in">
            <div class="field full">
              <label>代理地址</label>
              <input
                v-model="proxyValue"
                spellcheck="false"
                placeholder="例如 http://127.0.0.1:7890，留空为直连"
                @keydown.enter="commitProxy"
              />
            </div>

            <div class="field full">
              <label>任务日志级别</label>
              <select v-model="draft.log_level">
                <option v-for="item in LOG_LEVELS" :key="item.value" :value="item.value">
                  {{ item.label }}
                </option>
              </select>
            </div>

            <div class="field full">
              <label>数据目录</label>
              <div class="row-flex">
                <input
                  :value="dataDirValue"
                  spellcheck="false"
                  placeholder="留空时使用默认数据目录"
                  @input="dataDirDraft = $event.target.value; draft.data_dir = $event.target.value"
                />
                <button class="ghost" @click="chooseDataDir">选择</button>
              </div>
              <p class="note">
                登录凭据与日志都跟着这个目录走；改目录时会把现有的凭据复制过去（旧目录保留）。
                留空就用默认目录。
              </p>
            </div>

            <div class="btn-row full">
              <button class="ghost" :disabled="cleaning === 'temp'" @click="cleanup('temp')">
                清理临时文件
              </button>
              <button class="ghost" :disabled="cleaning === 'diag'" @click="cleanup('diag')">
                导出诊断
              </button>
            </div>
          </div>
        </div>
      </div>

      <p v-else class="loading">正在读取设置…</p>
    </section>
  </div>
</template>

<style scoped>
.card {
  padding: 18px 20px;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: var(--radius-lg);
}

.head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding-bottom: 14px;
  border-bottom: 1px solid var(--line);
}

h1 {
  margin: 0;
  font-size: 22px;
  font-weight: 700;
  letter-spacing: -0.2px;
}

.spacer {
  flex: 1;
}

.primary {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 8px 20px;
  font-weight: 600;
  color: #fff;
  background: var(--accent);
  border-radius: var(--radius-sm);
}

.primary:hover:not(:disabled) {
  background: var(--accent-dark);
}

.primary:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.primary svg {
  width: 18px;
  height: 18px;
}

.ghost {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 13px;
  font-size: 12.5px;
  color: var(--text);
  background: var(--field);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
}

.ghost svg {
  width: 17px;
  height: 17px;
  color: var(--muted);
}

/* 顶栏这三个（恢复默认 / 撤销 / 保存）小一号：它们是次要动作，别抢面板内容的戏 */
.head .primary,
.head .ghost {
  gap: 5px;
  padding: 6px 12px;
  font-size: 12px;
}

.head .primary {
  padding: 6px 14px;
}

.head .primary svg,
.head .ghost svg {
  width: 15px;
  height: 15px;
}

.ghost:hover:not(:disabled) {
  border-color: var(--accent-line);
  background: var(--raised);
}

.ghost:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.layout {
  display: flex;
  gap: 22px;
  margin-top: 18px;
  min-height: 420px;
}

/* 分类导航 */
.cats {
  flex: none;
  width: 190px;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.cats button {
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 9px 11px;
  text-align: left;
  color: var(--muted);
  border: 1px solid transparent;
  border-radius: var(--radius);
  transition: background 0.15s ease, color 0.15s ease;
}

.cats button:hover {
  background: var(--raised);
  color: var(--text);
}

.cats button.active {
  color: var(--accent-dark);
  background: var(--accent-soft);
  border-color: var(--accent-line);
}

.icon {
  flex: none;
  width: 22px;
  height: 22px;
}

.cats button.active .icon {
  color: var(--accent);
}

.text {
  display: flex;
  flex-direction: column;
  line-height: 1.3;
}

.label {
  font-size: 13px;
  font-weight: 600;
}

.cats .hint {
  font-size: 11px;
  color: var(--faint);
}

/* 面板 */
.panel {
  flex: 1;
  min-width: 0;
}

.panel-head {
  display: flex;
  align-items: center;
  gap: 11px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--line);
}

.panel-icon {
  flex: none;
  display: grid;
  place-items: center;
  width: 38px;
  height: 38px;
  color: var(--accent);
  background: var(--accent-soft);
  border-radius: var(--radius-sm);
}

h2 {
  margin: 0;
  font-size: 15px;
  font-weight: 700;
}

.panel-hint {
  margin: 1px 0 0;
  font-size: 11.5px;
  color: var(--faint);
}

.fields {
  padding-top: 14px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

/* 面板打开时抬升所在字段：字段因入场动画残留的内联 transform 会成为层叠上下文，
   把绝对定位的变量面板压到后续字段之下（实测踩过）——打开期间整体抬到最高 */
.field.lift {
  position: relative;
  z-index: 40;
}

.field label {
  font-size: 12.5px;
  font-weight: 600;
}

.grid2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
}

.full {
  width: 100%;
}

.row-flex {
  display: flex;
  gap: 8px;
  align-items: center;
}

.row-flex input:first-child {
  flex: 1;
}

input:not([type="checkbox"]),
select {
  padding: 8px 11px;
  color: var(--text);
  background: var(--field);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  font-size: 13px;
  transition: border-color 0.15s ease;
}

input:hover,
select:hover {
  border-color: var(--accent-line);
}

input:focus,
select:focus {
  outline: none;
  border-color: var(--accent-line);
  box-shadow: 0 0 0 3px var(--accent-soft);
}

input::placeholder {
  color: var(--faint);
}

.note {
  margin: 0;
  font-size: 11.5px;
  color: var(--faint);
  line-height: 1.6;
}

/* 提醒类说明：用警告色，和普通说明区分开 */
.note.warn {
  color: var(--warn);
}

/* 变量插入：按钮 + 面板共用定位上下文。
   z-index 建立独立层叠上下文：面板必须整体压过下方字段（深色同色会视觉穿透，实测踩过） */
.var-picker {
  position: relative;
  z-index: 30;
  flex: none;
}

.var-picker .ghost {
  padding: 8px 10px;
}

.var-picker .ghost.on {
  color: var(--accent);
  border-color: var(--accent-line);
}

.var-panel {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  z-index: 50;
  /* 栏目横排要的宽度；窗口窄的时候收一收，别顶到侧栏外面去 */
  width: min(644px, calc(100vw - 260px));
  padding: 10px;
  text-align: left;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: var(--radius);
  box-shadow: 0 16px 44px rgba(0, 0, 0, 0.5);
}

.var-head {
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 2px 4px 8px;
  font-size: 12.5px;
}

.var-head span {
  font-size: 11px;
  color: var(--faint);
}

/* 栏目横排：一列一个栏目，列内变量竖着堆 */
.var-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 10px 12px;
  align-items: start;
}

.var-col {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.var-col-name {
  margin: 0 0 3px;
  padding-bottom: 4px;
  border-bottom: 1px solid var(--line);
  font-size: 11.5px;
  font-weight: 600;
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.var-item {
  display: flex;
  flex-direction: column;
  min-width: 0;
  padding: 4px 7px;
  line-height: 1.25;
  text-align: left;
  border-radius: var(--radius-sm);
}

.var-item:hover {
  background: var(--hover);
}

.var-item code {
  font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
  font-size: 11.5px;
  line-height: 1.25;
  color: var(--accent);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.var-item span {
  font-size: 10.5px;
  line-height: 1.25;
  color: var(--faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.checks {
  display: flex;
  gap: 22px;
  flex-wrap: wrap;
}

.check {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
  cursor: pointer;
}

.check input {
  flex: none;
}

.card-check {
  padding: 11px 13px;
  background: var(--raised);
  border: 1px solid var(--line-soft);
  border-radius: var(--radius-sm);
}

.group {
  padding: 13px 14px;
  background: var(--raised);
  border: 1px solid var(--line-soft);
  border-radius: var(--radius);
}

.group-title {
  margin-bottom: 10px;
  font-size: 12.5px;
  font-weight: 700;
}

/* 提示图标：ASCII 的 ? + CSS 圆圈 */
.info {
  display: inline-grid;
  place-items: center;
  width: 15px;
  height: 15px;
  font-size: 9.5px;
  font-weight: 700;
  line-height: 1;
  color: var(--faint);
  border: 1px solid currentColor;
  border-radius: 50%;
  vertical-align: 0.5px;
  cursor: help;
}

.info:hover {
  color: var(--accent);
}

/* 登录状态：标题行（状态徽章）+ 内嵌小卡（图标 + 名称/UID 两行 + 动作文字链） */
.env {
  padding: 13px 14px;
  background: var(--raised);
  border: 1px solid var(--line-soft);
  border-radius: var(--radius);
}

.env-head {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
}

.env-title {
  font-size: 12.5px;
  font-weight: 700;
}

.env-title i {
  font-style: normal;
  font-weight: 400;
  color: var(--faint);
}

.env-recheck {
  flex: none;
  padding: 3px 10px;
  font: inherit;
  font-size: 12px;
  color: var(--accent-ink);
  background: none;
  border: none;
  cursor: pointer;
}

.env-recheck:hover {
  text-decoration: underline;
}

.env-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  background: var(--field);
  border: 1px solid var(--line-soft);
  border-radius: var(--radius-sm);
}

.env-row.bad {
  border-color: color-mix(in srgb, var(--err) 35%, var(--line-soft));
}

.env-icon {
  flex: none;
  width: 20px;
  height: 20px;
  color: var(--accent-ink);
}

.env-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.env-name {
  font-size: 12.5px;
  font-weight: 600;
}

.env-info {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11.5px;
  color: var(--faint);
}

.badge {
  padding: 1px 9px;
  font-size: 11px;
  font-weight: 600;
  border-radius: 999px;
}

.badge.ok {
  color: var(--ok);
  background: color-mix(in srgb, var(--ok) 14%, transparent);
}

.badge.bad {
  color: var(--err);
  background: color-mix(in srgb, var(--err) 14%, transparent);
}

.btn-row {
  display: flex;
  gap: 10px;
}

/* 开关（自动检测更新用） */
.switch {
  display: inline-flex;
  align-items: center;
  cursor: pointer;
  margin-top: 2px;
}

.switch input {
  display: none;
}

.switch .track {
  position: relative;
  width: 38px;
  height: 21px;
  background: var(--seg-idle);
  border-radius: 999px;
  transition: background 0.15s ease;
}

.switch .knob {
  position: absolute;
  top: 1px;
  left: 2px;
  width: 19px;
  height: 19px;
  background: #fff;
  border-radius: 50%;
  transition: transform 0.15s ease;
}

.switch input:checked + .track {
  background: var(--accent);
}

.switch input:checked + .track .knob {
  transform: translateX(16px);
}

/* 版本号 */
.version {
  margin: 0;
  font-size: 17px;
  font-weight: 700;
}

.note.top {
  margin-top: 4px;
}

.note.dim {
  color: var(--faint);
  opacity: 0.85;
}

.loading {
  margin: 22px 0 0;
  font-size: 12.5px;
  color: var(--faint);
}
</style>

<style scoped>
/* 收官动效：保存成功脉冲 */
.primary.saved-flash {
  animation: save-pulse 420ms var(--ease-out-expo);
}

@keyframes save-pulse {
  0% { transform: scale(0.95); }
  55% { transform: scale(1.04); }
  100% { transform: none; }
}
</style>
