<script setup>
// 嗅探页：本地代理的启停/证书/系统代理引导 + 命中清单 + 公众号捕获。
// 半自动"UP主"路线：在微信电脑版里打开目标公众号的历史文章页,
// 代理捕获会话凭据后,这里可以枚举该号全部文章并下载。
import { computed, onMounted, onUnmounted, ref } from "vue";
import Icon from "../components/Icon.vue";
import * as api from "../api";

const emit = defineEmits(["toast", "goto"]);

const status = ref({ running: false, port: 8899, cert_installed: false, sys_proxy_on: false, hits: 0, profiles: 0 });
const hits = ref([]);
const profiles = ref([]);
const selected = new Set();
const selectedHit = ref(0);
const busy = ref(false);
const portInput = ref(8899);

/** 公众号文章枚举结果（按 biz 存） */
const enumState = ref({});
/** 枚举出来的文章勾选 */
const enumSelected = ref(new Set());

let unlistenHit = null;
let unlistenProfile = null;

const ready = computed(() => status.value.running && status.value.cert_installed);

async function refresh() {
  try {
    status.value = await api.snifferStatus();
    hits.value = await api.snifferHits();
    profiles.value = await api.snifferProfiles();
    selected.clear();
    for (const hit of hits.value) {
      if (hit.kind !== "图片" && hit.url.length > 4) selected.add(hit.url);
    }
    selectedHit.value = selected.size;
  } catch (e) {
    emit("toast", String(e));
  }
}

async function startProxy() {
  busy.value = true;
  try {
    status.value = await api.snifferStart(portInput.value);
    emit("toast", `代理已启动：127.0.0.1:${status.value.port}`);
  } catch (e) {
    emit("toast", String(e));
  } finally {
    busy.value = false;
  }
}

async function stopProxy() {
  busy.value = true;
  try {
    status.value = await api.snifferStop();
    emit("toast", "代理已停止（系统代理已还原）");
  } catch (e) {
    emit("toast", String(e));
  } finally {
    busy.value = false;
  }
}

async function installCert() {
  busy.value = true;
  try {
    status.value = await api.snifferInstallCert();
    emit("toast", "根证书已安装进系统信任区");
  } catch (e) {
    emit("toast", String(e));
  } finally {
    busy.value = false;
  }
}

async function toggleSysProxy() {
  busy.value = true;
  try {
    status.value = await api.snifferSetSystemProxy(!status.value.sys_proxy_on);
    emit("toast", status.value.sys_proxy_on ? "系统代理已开启（退出应用自动还原）" : "系统代理已还原");
    if (status.value.sys_proxy_on) {
      emit("toast", "建议重启微信电脑版让代理生效");
    }
  } catch (e) {
    emit("toast", String(e));
  } finally {
    busy.value = false;
  }
}

function toggleHit(url) {
  if (selected.has(url)) selected.delete(url);
  else selected.add(url);
  selectedHit.value = selected.size;
}

function toggleAllHits(on) {
  selected.clear();
  if (on) for (const hit of hits.value) if (hit.kind !== "图片") selected.add(hit.url);
  selectedHit.value = selected.size;
}

async function downloadSelected() {
  const picked = hits.value.filter((h) => selected.has(h.url));
  if (!picked.length) {
    emit("toast", "先勾选要下载的资源");
    return;
  }
  try {
    const n = await api.snifferDownloadMedia(picked);
    emit("toast", `已加入 ${n} 个下载任务`);
    emit("goto", "transfer");
  } catch (e) {
    emit("toast", String(e));
  }
}

async function copyHit(url) {
  try {
    await navigator.clipboard.writeText(url);
    emit("toast", "地址已复制");
  } catch {
    emit("toast", "复制失败");
  }
}

async function clearHits() {
  await api.snifferClear();
  hits.value = [];
  selected.clear();
  selectedHit.value = 0;
}

/** 枚举公众号文章（第一次 3 页，之后每次续 5 页） */
async function enumArticles(biz, first) {
  const state = enumState.value[biz] || { items: [], more: false, loading: false, note: "" };
  state.loading = true;
  enumState.value = { ...enumState.value, [biz]: state };
  try {
    const result = await api.snifferProfileEnum(biz, first ? 3 : 5);
    // 追加去重
    const seen = new Set(state.items.map((i) => i.url));
    const merged = [...state.items];
    for (const item of result.items) {
      if (!seen.has(item.url)) {
        merged.push(item);
        seen.add(item.url);
      }
    }
    merged.sort((a, b) => (b.create_time || 0) - (a.create_time || 0));
    state.items = merged;
    state.more = result.more;
    state.note = result.note;
    enumSelected.value = new Set(merged.map((i) => i.url));
  } catch (e) {
    state.note = String(e);
  } finally {
    state.loading = false;
    enumState.value = { ...enumState.value, [biz]: state };
  }
}

function toggleEnum(biz, url) {
  const state = enumState.value[biz];
  if (!state) return;
  if (enumSelected.value.has(url)) enumSelected.value.delete(url);
  else enumSelected.value.add(url);
  enumSelected.value = new Set(enumSelected.value);
}

/** 下载勾选的文章：走文章管线（正文/图片/存档） */
async function downloadArticles(biz) {
  const state = enumState.value[biz];
  if (!state) return;
  const picked = state.items.filter((i) => enumSelected.value.has(i.url));
  if (!picked.length) {
    emit("toast", "先勾选要下载的文章");
    return;
  }
  let started = 0;
  for (const [index, item] of picked.entries()) {
    const pad = String(picked.length).length;
    try {
      await api.startDownload({
        bid: "",
        mid: "",
        source: "article",
        collection: "",
        fallback_text: item.digest || "",
        title: item.title.slice(0, 30),
        author: `公众号${biz.slice(-6)}`,
        uid: biz,
        quality: "auto",
        image: "large",
        audio_quality: "best",
        is_audio: false,
        media_url: "",
        page_url: item.url,
        image_count: 0,
        video_count: 0,
        quality_label: "文章",
        naming: {
          author: `公众号${biz.slice(-6)}`,
          uid: biz,
          bid: "",
          mid: "",
          collection: "",
          publish_date: localDate(item.create_time),
          date: localDate(Math.floor(Date.now() / 1000)),
          year: String(new Date((item.create_time || Date.now() / 1000) * 1000).getFullYear()),
          month: String(new Date((item.create_time || Date.now() / 1000) * 1000).getMonth() + 1).padStart(2, "0"),
          source_kind: "公众号全部文章",
          index: index + 1,
          index_pad: Math.max(pad, 2),
        },
      });
      started += 1;
    } catch (e) {
      emit("toast", `${item.title.slice(0, 16)}: ${e}`);
    }
  }
  if (started) {
    emit("toast", `已加入 ${started} 篇文章下载`);
    emit("goto", "transfer");
  }
}

function localDate(unixSecs) {
  const d = unixSecs ? new Date(unixSecs * 1000) : new Date();
  const pad = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function fmtTime(ts) {
  if (!ts) return "—";
  const d = new Date(ts * 1000);
  const pad = (n) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

onMounted(async () => {
  await refresh();
  unlistenHit = await api.onSniffHit(() => {
    // 命中事件只做轻量刷新(列表由全量接口给,避免高频事件撑爆界面)
    refreshSoon();
  });
  unlistenProfile = await api.onSniffProfile(() => {
    refreshSoon();
  });
});

let refreshTimer = null;
function refreshSoon() {
  if (refreshTimer) return;
  refreshTimer = setTimeout(async () => {
    refreshTimer = null;
    try {
      status.value = await api.snifferStatus();
      hits.value = await api.snifferHits();
      profiles.value = await api.snifferProfiles();
      for (const hit of hits.value) {
        if (!selected.has(hit.url) && hit.kind !== "图片") selected.add(hit.url);
      }
      selectedHit.value = selected.size;
    } catch {}
  }, 600);
}

onUnmounted(() => {
  if (unlistenHit) unlistenHit();
  if (unlistenProfile) unlistenProfile();
  if (refreshTimer) clearTimeout(refreshTimer);
});
</script>

<template>
  <section class="card">
    <h1>嗅探</h1>
    <p class="lead">
      本地代理捕获微信流量：小程序/视频号里播放的<b>视频和图片</b>、在微信电脑版里浏览的<b>公众号历史文章</b>（自动捕获会话，可枚举全部文章——"UP主"下载）。
    </p>

    <!-- 状态行 -->
    <div class="row status-row">
      <span class="pill" :class="status.running ? 'ok' : 'off'">
        代理 {{ status.running ? `运行中 · ${status.port}` : "已停止" }}
      </span>
      <span class="pill" :class="status.cert_installed ? 'ok' : 'warn'">
        根证书 {{ status.cert_installed ? "已安装" : "未安装" }}
      </span>
      <span class="pill" :class="status.sys_proxy_on ? 'ok' : 'off'">
        系统代理 {{ status.sys_proxy_on ? "已接管" : "未开启" }}
      </span>
      <span class="spacer"></span>
      <input v-model="portInput" class="port" type="number" min="1024" max="65535" :disabled="status.running" />
      <button v-if="!status.running" class="ghost" :disabled="busy" @click="startProxy">启动代理</button>
      <button v-else class="ghost" :disabled="busy" @click="stopProxy">停止代理</button>
      <button v-if="!status.cert_installed" class="ghost" :disabled="busy" @click="installCert">安装根证书</button>
      <button
        class="ghost"
        :disabled="busy || !status.running"
        :title="status.running ? '' : '先启动代理'"
        @click="toggleSysProxy"
      >
        {{ status.sys_proxy_on ? "还原系统代理" : "接管系统代理" }}
      </button>
    </div>

    <!-- 引导 -->
    <div v-if="!ready || !status.sys_proxy_on" class="guide">
      <p class="hint">
        三步开启：<b>① 启动代理 → ② 安装根证书（弹系统确认框，点"是"）→ ③ 接管系统代理</b>，
        然后<b>重启微信电脑版</b>。在微信里播放小程序视频、或打开某公众号的历史文章页，命中就会出现在下面。
        退出 WXdown 或点"还原系统代理"会自动恢复网络设置。
      </p>
    </div>

    <!-- 公众号捕获 -->
    <template v-if="profiles.length">
      <h2 class="sec-title">公众号会话（在微信里打开其历史文章页即捕获）</h2>
      <div v-for="p in profiles" :key="p.biz" class="profile-card card-flat">
        <div class="row">
          <Icon name="user" />
          <b>公众号 {{ p.biz }}</b>
          <span class="faint">{{ p.action }} · {{ fmtTime(p.ts) }}</span>
          <span class="spacer"></span>
          <button class="ghost" :disabled="(enumState[p.biz] || {}).loading" @click="enumArticles(p.biz, !(enumState[p.biz] || {}).items?.length)">
            {{ (enumState[p.biz] || {}).items?.length ? "继续加载" : "枚举全部文章" }}
          </button>
          <button
            v-if="(enumState[p.biz] || {}).items?.length"
            class="primary"
            @click="downloadArticles(p.biz)"
          >
            下载所选（{{ enumSelected.size }}）
          </button>
        </div>
        <p v-if="(enumState[p.biz] || {}).note" class="hint warn-text">{{ enumState[p.biz].note }}</p>
        <div v-if="(enumState[p.biz] || {}).items?.length" class="table-scroll">
          <table class="batch-table">
            <thead>
              <tr>
                <th class="col-check">
                  <input
                    type="checkbox"
                    :checked="enumSelected.size === enumState[p.biz].items.length"
                    @change="(e) => (enumSelected = e.target.checked ? new Set(enumState[p.biz].items.map((i) => i.url)) : new Set())"
                  />
                </th>
                <th class="col-idx">序号</th>
                <th>标题</th>
                <th class="col-time">发布时间</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(item, i) in enumState[p.biz].items" :key="item.url">
                <td class="col-check">
                  <input type="checkbox" :checked="enumSelected.has(item.url)" @change="toggleEnum(p.biz, item.url)" />
                </td>
                <td class="col-idx num">{{ String(i + 1).padStart(2, "0") }}</td>
                <td class="col-title" :title="item.digest">{{ item.title }}</td>
                <td class="col-time num">{{ item.create_time ? localDate(item.create_time) : "—" }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </template>

    <!-- 媒体命中 -->
    <h2 class="sec-title">媒体命中（{{ hits.length }}）</h2>
    <div class="row">
      <label class="mini-line">
        <input type="checkbox" :checked="selectedHit > 0 && selectedHit === hits.filter((h) => h.kind !== '图片').length" @change="(e) => toggleAllHits(e.target.checked)" />
        全选非图片
      </label>
      <span class="faint">已选 {{ selectedHit }}</span>
      <span class="spacer"></span>
      <button class="primary" :disabled="!selectedHit" @click="downloadSelected">下载所选</button>
      <button class="ghost" @click="clearHits">清空</button>
    </div>
    <p v-if="!hits.length" class="hint pad">
      还没有命中：开启代理并重启微信后，播放小程序视频/浏览图片就会出现在这里。
    </p>
    <div v-else class="table-scroll">
      <table class="batch-table">
        <thead>
          <tr>
            <th class="col-check"></th>
            <th class="col-type">类型</th>
            <th>地址</th>
            <th class="col-time">时间</th>
            <th class="col-op">操作</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="hit in hits" :key="hit.url" :class="{ on: selected.has(hit.url) }">
            <td class="col-check">
              <input type="checkbox" :checked="selected.has(hit.url)" @change="toggleHit(hit.url)" />
            </td>
            <td class="col-type">
              <span class="kind-tag" :class="{ live: hit.kind === '直播流' }">{{ hit.kind }}</span>
            </td>
            <td class="col-title mono" :title="hit.url">{{ hit.host }}/{{ hit.url.split("/").slice(3).join("/").slice(0, 64) }}</td>
            <td class="col-time num">{{ fmtTime(hit.ts) }}</td>
            <td class="col-op">
              <button class="mini" @click="copyHit(hit.url)">复制</button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>

<style scoped>
.status-row {
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.pill {
  padding: 4px 12px;
  border-radius: 999px;
  font-size: 12.5px;
  border: 1px solid var(--line);
}
.pill.ok {
  color: var(--accent-ink);
  background: var(--accent-soft);
  border-color: var(--accent-line);
}
.pill.warn {
  color: #b06a00;
  background: #fff6e5;
  border-color: #f2d9a8;
}
.pill.off {
  color: var(--text-faint);
}
.port {
  width: 88px;
  padding: 6px 8px;
}
.guide {
  margin: 10px 0 4px;
  padding: 10px 14px;
  border: 1px dashed var(--accent-line);
  border-radius: 10px;
  background: var(--accent-soft);
}
.sec-title {
  margin: 22px 0 8px;
  font-size: 15px;
}
.profile-card {
  padding: 10px 14px;
  margin-bottom: 10px;
  border: 1px solid var(--line);
  border-radius: 10px;
}
.card-flat {
  background: var(--card);
}
.warn-text {
  color: #b06a00;
}
.mini-line {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
}
.kind-tag {
  font-size: 12px;
  padding: 2px 8px;
  border-radius: 6px;
  background: var(--accent-soft);
  color: var(--accent-ink);
}
.kind-tag.live {
  background: #fdecec;
  color: #c4132c;
}
.mono {
  font-family: ui-monospace, Consolas, monospace;
  font-size: 12px;
}
.col-op {
  width: 70px;
}
.pad {
  padding: 12px 4px;
}
</style>
