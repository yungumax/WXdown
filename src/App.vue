<script setup>
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { animate, stagger } from "animejs";

import * as api from "./api";
import TitleBar from "./components/TitleBar.vue";
import Sidebar from "./components/Sidebar.vue";
import LoginDialog from "./components/LoginDialog.vue";
import Onboarding from "./components/Onboarding.vue";
import ParsePage from "./pages/ParsePage.vue";
import TransferPage from "./pages/TransferPage.vue";
import SettingsPage from "./pages/SettingsPage.vue";
import AboutPage from "./pages/AboutPage.vue";
import LibraryPage from "./pages/LibraryPage.vue";

const RUNNING = ["queued", "downloading", "saving"];

const page = ref("parse");

/** 切页编排：新页可见卡的直接子元素（标题/工具条/列表）依次浮起。
    根元素仍走 .page-in 淡入，两者不冲突（不同元素）。
    只动 transform/opacity，零视觉变化；减少动态下不演。 */
watch(page, async () => {
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  await nextTick();
  requestAnimationFrame(() => {
    const card = [...document.querySelectorAll(".content .card")].find((c) => c.offsetParent !== null);
    if (!card) return;
    const kids = [...card.children].filter((el) => getComputedStyle(el).position !== "fixed").slice(0, 10);
    if (!kids.length) return;
    animate(kids, {
      opacity: [0, 1],
      translateY: [10, 0],
      duration: 460,
      delay: stagger(45),
      ease: "outExpo",
    }).then(() => {
      // 动画完成后清掉内联 transform：恒等矩阵也会创建层叠上下文，
      // 把字段里的绝对定位弹层（每批/下载设置等）压到后续字段之下（实测踩过）
      kids.forEach((k) => (k.style.transform = ""));
    });
  });
});
const login = ref({ logged_in: false, uname: "", face: "", mid: "", vip: false, vip_label: "" });
const version = ref("");
const outputDir = ref("");
const tasks = ref([]);
const toastText = ref("");
const showLogin = ref(false);
const settings = ref(null);
const settingsEnv = ref(null);
const showOnboarding = ref(false);

let unlistenTask = null;
let unlistenLogin = null;
let toastTimer = null;

const queue = computed(() => {
  const running = tasks.value.filter((task) => RUNNING.includes(task.status));
  return {
    active: running.length,
    speed: running.reduce((sum, task) => sum + (task.speed_bps || 0), 0),
  };
});

onMounted(async () => {
  // 首帧之后再开颜色过渡：启动时要的是"立刻正确"，不是"渐变色"
  requestAnimationFrame(() => document.documentElement.classList.add("ready"));
  applyUiScale();
  window.addEventListener("resize", applyUiScale);
  // 启动编排（收官动效）：顶栏轻落、侧栏导航逐项浮进——与应用页的 page-in
  // 组成完整的"开机画面"；reduce 下不演
  if (!matchMedia("(prefers-reduced-motion: reduce)").matches) {
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        animate(".titlebar", { opacity: [0, 1], translateY: [-6, 0], duration: 380, ease: "outExpo" });
        animate(".sidebar nav .nav-item", { opacity: [0, 1], translateX: [-10, 0], duration: 440, delay: stagger(55), ease: "outExpo" });
      }),
    );
  }
  try {
    const status = await api.appStatus();
    login.value = status.login;
    outputDir.value = status.output_dir;
    version.value = status.version;
    api.uiLog(
      `启动登录态：${login.value.logged_in ? "已登录" : "未登录"} 昵称=${login.value.uname || "空"} 头像=${login.value.face ? "有" : "无"}`,
    );
  } catch (error) {
    api.uiLog(`启动取状态失败：${error}`);
    showToast(String(error));
  }

  await loadSettings();

  // 自动检测更新（设置里开了才查）：发现新版只提示，不自动下载
  if (settings.value?.update_check) {
    api
      .checkUpdates()
      .then((result) => {
        if (result && !result.up_to_date && result.latest) {
          showToast(`发现新版本 v${result.latest}，到「设置 · 应用更新」查看`);
        }
      })
      .catch(() => {});
  }

  // 上次没下完的任务：设置里开了「启动时自动继续」就接着下（分片记录让已下载的字节不重下）
  if (settings.value?.resume_on_start) {
    api
      .resumePending()
      .then((count) => {
        if (count > 0) showToast(`继续未完成的下载：${count} 个任务`);
      })
      .catch(() => {});
  }

  unlistenTask = await api.onTaskUpdate((task) => {
    const index = tasks.value.findIndex((item) => item.id === task.id);
    if (index === -1) tasks.value.unshift(task);
    else tasks.value[index] = task;
  });

  // 登录成功的通知（后端检测到登录后推送）：全局监听——登录窗是独立的官方页面，
  // 应用侧不留自定义登录弹窗，收到通知只更新右上角并把结果告诉用户
  unlistenLogin = await api.onLoginConfirmed((info) => {
    onWebConfirmed(info);
  });

});

onUnmounted(() => {
  if (unlistenTask) unlistenTask();
  if (unlistenLogin) unlistenLogin();
  clearTimeout(toastTimer);
});

/**
 * 把主题模式写到根元素上：light / dark 直接生效，
 * system 交给 CSS 的 prefers-color-scheme media query。
 * 注意：启动首帧不依赖这里——那是 Rust 初始化脚本的职责，否则会先看到一次配色翻转。
 * 切换瞬时完成；同步窗口原生底色，避免边缘/滚动条区域露出旧色。
 */
function applyTheme(mode) {
  document.documentElement.dataset.theme = mode;
  // system 模式的原生底色由 Rust 启动时按真实系统主题设置，前端不干预；
  // 显式切换时同步，避免边缘露出旧色
  if (mode !== "system") {
    api.setWindowBackground(mode === "dark" ? "#0f1011" : "#f5f3f4");
  }
}

/** 整体 UI 随窗口等比缩放：以默认窗口 1100×740 为 1.0，取宽高比较小的一边
 *  （避免只拉一边时另一边溢出），夹在 0.85–1.3，并按 0.05 量化——
 *  拖拽调节大小时不会逐帧跳变。默认尺寸下恰好 1.0，仪器断言不受影响。 */
function applyUiScale() {
  const raw = Math.min(window.innerWidth / 1100, window.innerHeight / 740);
  const zoom = Math.round(Math.min(1.3, Math.max(0.85, raw)) * 20) / 20;
  document.documentElement.style.zoom = String(zoom);
}

async function loadSettings() {
  try {
    const data = await api.appSettings();
    settingsEnv.value = data;
    settings.value = data.settings;
    applyTheme(data.settings.theme);
    // 首启引导：安装后第一次打开（settings 里 onboarded=false）时显示
    showOnboarding.value = !data.settings.onboarded;
  } catch (error) {
    showToast(String(error));
  }
}

/** 引导页完成：保存引导收集的设置（含 onboarded=true） */
async function finishOnboarding(payload) {
  showOnboarding.value = false;
  await saveSettings(payload);
}

/** 内容库点「去解析」时带过来的来源：设置后切到解析页，那边看到就自动解析 */
const pendingSource = ref(null);

function openSource(url, autoParse = true) {
  pendingSource.value = { url, stamp: Date.now(), autoParse };
  page.value = "parse";
}

async function saveSettings(next) {
  try {
    const data = await api.updateSettings(next);
    settingsEnv.value = data;
    settings.value = data.settings;
    applyTheme(data.settings.theme);
    showToast("设置已保存");
  } catch (error) {
    showToast(String(error));
    await loadSettings();
  }
}

/** 主题由标题栏的菜单选定；切换即时保存 */
async function setTheme(theme) {
  if (!settings.value) return;
  const next = { ...settings.value, theme };
  try {
    const data = await api.updateSettings(next);
    settingsEnv.value = data;
    settings.value = data.settings;
    applyTheme(data.settings.theme);
  } catch (error) {
    showToast(String(error));
    await loadSettings();
  }
}

const DEFAULT_SETTINGS = {
  max_concurrent_tasks: 2,
  chunk_concurrency: 4,
  chunk_mb: 4,
  keep_temp: false,
  naming_template: "{index} {title}.{ext}",
  naming_presets: [],
  folder_template: "{author}/{collection}",
  folder_presets: [],
  rename_conflict: "skip",
  image_format: "large",
  video_quality: "auto",
  audio_quality: "best",
  download_text: true,
  retry_count: 3,
  speed_limit_mib: 0,
  resume_on_start: false,
  parse_preset: "标准",
  parse_batch: 8,
  parse_batch_wait_ms: 1000,
  parse_rest_every: 100,
  parse_rest_ms: 3000,
  log_level: "info",
  data_dir: "",
  update_check: false,
  onboarded: true,
  proxy: "",
  theme: "system",
};

async function resetSettings() {
  if (!settings.value) return;
  await saveSettings({ ...DEFAULT_SETTINGS, output_dir: settings.value.output_dir });
  showToast("已恢复默认设置（保存位置不变）");
}

function showToast(text) {
  if (!text) return;
  toastText.value = text;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (toastText.value = ""), 3600);
}

async function cancelTask(taskId) {
  try {
    await api.cancelDownload(taskId);
  } catch (error) {
    showToast(String(error));
  }
}

async function openPath(path) {
  try {
    await api.openPath(path);
  } catch (error) {
    showToast(String(error));
  }
}

async function pauseAll() {
  try {
    await api.pauseAllDownloads();
  } catch (error) {
    showToast(String(error));
  }
}

async function resumeAll() {
  try {
    await api.resumeAllDownloads();
  } catch (error) {
    showToast(String(error));
  }
}

async function pauseTask(taskId) {
  try {
    await api.pauseDownload(taskId);
  } catch (error) {
    showToast(String(error));
  }
}

async function resumeTask(taskId) {
  try {
    await api.resumeDownload(taskId);
  } catch (error) {
    showToast(String(error));
  }
}

async function revealPath(path) {
  try {
    await api.revealPath(path);
  } catch (error) {
    showToast(String(error));
  }
}

function clearFinished() {
  tasks.value = tasks.value.filter((task) => RUNNING.includes(task.status));
}

/** 网页登录收割成功：写登录态 + 刷新界面 */
function onWebConfirmed(loginInfo) {
  login.value = loginInfo;
  api.uiLog(
    `登录事件收到：昵称=${loginInfo.uname || "空"} 头像=${loginInfo.face ? "有" : "无"}`,
  );
  // logged_in=false = 用户在网页里自行退出了（登录检查点发现并广播）
  showToast(loginInfo.logged_in ? `已登录：${loginInfo.uname || "微信账号"}` : "已退出登录");
  showLogin.value = false;
}

async function openLogin() {
  if (login.value.logged_in) {
    showLogin.value = true; // 已登录时打开的是账号面板（看昵称头像、退出登录）
    return;
  }
  try {
    await api.webLoginOpen();
    await api.webLoginWatch();
    showToast("已发起登录，登录成功后这里会自动完成绑定");
  } catch (error) {
    showToast(String(error));
  }
}

async function doLogout() {
  try {
    login.value = await api.logout();
    showLogin.value = false;
    showToast("已退出登录");
  } catch (error) {
    showToast(String(error));
  }
}
</script>

<template>
  <div class="app">
    <TitleBar
      :login="login"
      :version="version"
      :theme="settings?.theme || 'system'"
      @login="openLogin"
      @set-theme="setTheme"
    />

    <div class="body">
      <Sidebar
        :current="page"
        :login="login"
        :queue="queue"
        @navigate="page = $event"
      />

      <main class="content">
        <!-- 解析页用 v-show 常驻：切到别的栏目再回来，已解析的清单与勾选都还在。
             其余页面按需挂载，所以这里不用 v-else-if 链。 -->
        <ParsePage
          v-show="page === 'parse'"
          :class="{ 'page-in': page === 'parse' }"
          :login="login"
          :settings="settings"
          @toast="showToast"
          @goto="page = $event"
          :pending-source="pendingSource"
        />
        <TransferPage
          v-if="page === 'transfer'"
          :class="{ 'page-in': page === 'transfer' }"
          :tasks="tasks"
          @cancel="cancelTask"
          @open="openPath"
          @reveal="revealPath"
          @pause="pauseTask"
          @resume="resumeTask"
          @pause-all="pauseAll"
          @resume-all="resumeAll"
          @clear="clearFinished"
        />
        <SettingsPage
          v-if="page === 'settings'"
          :class="{ 'page-in': page === 'settings' }"
          :login="login"
          :settings="settings"
          :env="settingsEnv"
          @toast="showToast"
          @login="openLogin"
          @logout="doLogout"
          @save="saveSettings"
          @reload="loadSettings"
          @reset="resetSettings"
        />
        <AboutPage v-if="page === 'about'" class="page-in" :version="version" />
        <LibraryPage
          v-if="page === 'library'"
          class="page-in"
          :login="login"
          @goto="page = $event"
          @open-source="openSource"
          @login="showLogin = true"
        />
      </main>
    </div>

    <Transition name="toast">
      <div v-if="toastText" class="toast">{{ toastText }}</div>
    </Transition>

    <Onboarding
      v-if="showOnboarding && settings"
      :settings="settings"
      :version="settingsEnv?.version || ''"
      @done="finishOnboarding"
    />

    <LoginDialog
      v-if="showLogin"
      :login="login"
      @close="showLogin = false"
      @logout="doLogout"
      @web-confirmed="onWebConfirmed"
    />
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.body {
  flex: 1;
  min-height: 0;
  display: flex;
}

.content {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  padding: 18px 22px 22px;
}

.toast {
  position: fixed;
  left: 50%;
  bottom: 26px;
  transform: translateX(-50%);
  padding: 9px 18px;
  font-size: 12.5px;
  background: var(--text);
  color: var(--card);
  border-radius: 999px;
  box-shadow: 0 8px 24px rgba(66, 24, 32, 0.2);
  z-index: 30;
}

.toast-enter-active {
  transition: opacity 0.26s var(--ease-out-expo), transform 0.26s var(--ease-out-expo);
}

.toast-leave-active {
  transition: opacity 0.16s ease, transform 0.16s ease;
}

.toast-enter-from {
  opacity: 0;
  transform: translate(-50%, 14px) scale(0.96);
}

.toast-leave-to {
  opacity: 0;
  transform: translate(-50%, 6px) scale(0.98);
}
</style>
