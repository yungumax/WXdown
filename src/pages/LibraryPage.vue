<script setup>
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { animate, stagger } from "animejs";
import Icon from "../components/Icon.vue";
import * as api from "../api";

const props = defineProps({
  login: { type: Object, required: true },
});
const emit = defineEmits(["goto", "open-source", "login"]);

const loading = ref(false);
const error = ref("");
const account = ref(null);
const query = ref("");
const picked = ref(new Set());

const loggedIn = computed(() => !!props.login?.logged_in);

const list = computed(() => {
  const items = account.value?.created ?? [];
  const keyword = query.value.trim().toLowerCase();
  if (!keyword) return items;
  return items.filter((item) => (item.title ?? "").toLowerCase().includes(keyword));
});

/** 勾选可以跨"搜索"：取来源时按全量列表取，不只看当前过滤出的那一列 */
const allItems = computed(() => account.value?.created ?? []);

const keyOf = (item) => `${item.kind || "fav"}:${item.id}`;

/** 条目对应的本地文件夹路径（点卡片打开） */
const itemUrl = (item) => item.url || "";

function subLabel(item) {
  return `已下载合集 · ${formatTime(item.created_at)}`;
}

function countLabel(item) {
  return item.video ? "视频" : "图文";
}

function formatTime(unixSecs) {
  if (!unixSecs) return "—";
  const d = new Date(unixSecs * 1000);
  const pad = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

const allPicked = computed(
  () => list.value.length > 0 && list.value.every((item) => picked.value.has(keyOf(item)))
);

function toggleAllPicked() {
  const next = new Set(picked.value);
  if (allPicked.value) {
    list.value.forEach((item) => next.delete(keyOf(item)));
  } else {
    list.value.forEach((item) => next.add(keyOf(item)));
  }
  picked.value = next;
}

async function load() {
  if (loading.value) return;
  loading.value = true;
  error.value = "";
  try {
    account.value = await api.libraryFolders();
    picked.value = new Set();
  } catch (e) {
    error.value = String(e);
    account.value = null;
  } finally {
    loading.value = false;
  }
}

function togglePick(key) {
  const next = new Set(picked.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  picked.value = next;
}

/** 点卡片：本地文件夹直接打开；其余交给解析页按链接解析 */
async function openItem(item) {
  const url = itemUrl(item);
  if (/^[A-Za-z]:[\/]|^\\/.test(url)) {
    try {
      await api.openPath(url);
    } catch (e) {
      console.warn("打开文件夹失败", e);
    }
    return;
  }
  emit("open-source", url);
}

/** 选中若干条（可跨搜索过滤）→ 多行一起交给解析页 */
function openPicked() {
  const selected = allItems.value.filter((item) => picked.value.has(keyOf(item)));
  if (!selected.length) return;
  emit("open-source", selected.map(itemUrl).join("\n"));
}

// 本地下载记录不依赖登录态：直接读
watch(
  () => props.login?.logged_in,
  () => {
    /* 阶段一：内容库=本地扫描，与登录无关；阶段二接入公众号库后再区分 */
  },
  { immediate: true }
);

onMounted(() => {
  load();
});

/** 卡片 3D 微倾斜（±2.4°）：指针在哪边卡片就朝哪边轻轻转，移开复位。
    倾斜写在卡片自己的 transform 上（含悬停那 1px 上浮），纯动效零视觉。 */
let tiltedCard = null;

function onTilt(event) {
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const card = event.target instanceof Element ? event.target.closest(".collection") : null;
  if (!card) return;
  if (tiltedCard && tiltedCard !== card) tiltedCard.style.transform = "";
  tiltedCard = card;
  const r = card.getBoundingClientRect();
  const px = (event.clientX - r.left) / r.width - 0.5;
  const py = (event.clientY - r.top) / r.height - 0.5;
  card.style.transform = `translateY(-1px) rotateX(${(-py * 2.4).toFixed(2)}deg) rotateY(${(px * 2.4).toFixed(2)}deg)`;
}
function offTilt() {
  // pointerleave 绑在容器上，target 是容器不是卡——按记录复位
  if (tiltedCard) {
    tiltedCard.style.transform = "";
    tiltedCard = null;
  }
}

// 入场：卡片首次渲染出来后逐张浮起（一次性编排，减少动态下不演）
let roseIn = false;
watch(
  () => list.value.length,
  async (n) => {
    if (!n || roseIn) return;
    roseIn = true;
    await nextTick();
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    animate(".card .collection", {
      opacity: [0, 1],
      translateY: [16, 0],
      duration: 520,
      delay: stagger(60),
      ease: "outExpo",
    });
  },
);
</script>

<template>
  <div>
    <section class="card">
      <header class="head">
        <div>
          <h2>下载记录</h2>
          <p class="meta">按 公众号 / 合集 统计的本地下载记录 · 点卡片打开文件夹</p>
          <p v-if="account?.fav_note" class="meta dim">{{ account.fav_note }}</p>
        </div>
        <span class="grow"></span>
        <button v-if="list.length" class="ghost" @click="toggleAllPicked">
          {{ allPicked ? "取消全选" : "全部选择" }}
        </button>
        <button class="ghost" :disabled="loading" title="刷新" @click="load">
          <Icon name="refresh" class="btn-icon" :class="{ spin: loading }" />
        </button>
      </header>

      <!-- 阶段一：本地下载记录（不需要登录） -->
      <div v-if="false" class="empty tall">
        <Icon name="lock" class="empty-icon" />
        <div class="empty-text">
          <p class="title">登录后连接你的内容库</p>
          <p class="hint">
            登录凭据只存在本机（数据目录里的 cookies.json），内容库不会显示或导出 Cookie 的内容。
          </p>
        </div>
        <button class="ghost" @click="load">刷新</button>
      </div>

      <template v-else>
        <div class="search">
          <Icon name="search" class="search-icon" />
          <input v-model="query" spellcheck="false" placeholder="搜索公众号或合集" />
        </div>

        <p v-if="loading && !account" class="hint pad">正在读取下载记录…</p>
        <div v-else-if="error" class="empty">
          <div class="empty-text">
            <p class="title">没读出来</p>
            <p class="hint">{{ error }}</p>
          </div>
          <button class="ghost" @click="load">重试</button>
        </div>

        <div v-else-if="list.length" class="cards page-in" @pointermove="onTilt" @pointerleave="offTilt">
          <article
            v-for="item in list"
            :key="keyOf(item)"
            class="collection"
            :class="{ on: picked.has(keyOf(item)) }"
            @click="openItem(item)"
          >
            <div class="thumb">
              <div class="thumb-count">
                <Icon :name="item.video ? 'play' : 'photo'" />
                <b>{{ item.video ? "视频" : "图文" }}</b>
              </div>
            </div>
            <div class="body">
              <p class="ctitle" :title="item.title">{{ item.title }}</p>
              <p class="sub">{{ subLabel(item) }}</p>
              <p class="num">{{ countLabel(item) }}</p>
            </div>
            <button
              class="pick"
              :title="picked.has(keyOf(item)) ? '取消选择' : '选择这条'"
              @click.stop="togglePick(keyOf(item))"
            >
              <Icon :name="picked.has(keyOf(item)) ? 'check' : 'plus'" />
            </button>
          </article>
        </div>

        <div v-else class="empty">
          <div class="empty-text">
            <p class="title">
              {{ query ? "没有匹配的记录" : "还没有下载记录" }}
            </p>
            <p class="hint">
              {{ query ? "换个关键词试试。" : "解析一篇文章或合集开始使用。" }}
            </p>
          </div>
        </div>

        <footer v-if="list.length" class="foot">
          <span class="count">已选 {{ picked.size }} 条</span>
          <button class="primary" :disabled="!picked.size" @click="openPicked">解析所选来源</button>
        </footer>
      </template>
    </section>
  </div>
</template>

<style scoped>
.card {
  padding: 18px 22px 20px;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: var(--radius-lg);
}

.head {
  display: flex;
  align-items: center;
  gap: 10px;
}

.grow {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

h2 {
  margin: 0;
  font-size: 20px;
  font-weight: 700;
  letter-spacing: -0.2px;
}

.meta {
  margin: 5px 0 0;
  font-size: 12.5px;
  color: var(--muted);
}

.meta.dim {
  margin-top: 3px;
  font-size: 11.5px;
  color: var(--faint);
}

.btn-icon {
  width: 16px;
  height: 16px;
}

.search {
  position: relative;
  margin-top: 14px;
}

.search-icon {
  position: absolute;
  top: 50%;
  left: 11px;
  width: 16px;
  height: 16px;
  color: var(--faint);
  transform: translateY(-50%);
}

.search input {
  width: 100%;
  padding: 9px 12px 9px 34px;
  font: inherit;
  font-size: 13px;
  color: var(--text);
  background: var(--field);
  border: 1px solid var(--line);
  border-radius: var(--radius);
  transition: border-color var(--motion-fast) var(--ease-out);
}

.search input:focus-visible {
  outline: none;
  border-color: var(--accent-line);
  box-shadow: 0 0 0 3px var(--accent-soft);
}

.pad {
  margin: 16px 0 0;
}

.cards {
  perspective: 640px;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  /* 卡片之间的间隔略微放大，透气一点 */
  gap: 18px;
  margin-top: 16px;
}

.collection {
  position: relative;
  display: flex;
  gap: 13px;
  padding: 12px;
  background: var(--card);
  /* 悬停反馈：边框变色 + 1px 上浮，平面语言里的一点"可点" */
  transition:
    background var(--motion-fast) var(--ease-out),
    border-color var(--motion-fast) var(--ease-out),
    transform var(--motion-fast) var(--ease-out);
  border: 1px solid var(--line);
  border-radius: var(--radius);
  cursor: pointer;
}

.collection:hover {
  border-color: var(--accent-line);
  transform: translateY(-1px);
}

.collection.on {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.thumb {
  flex: none;
  width: 92px;
  height: 62px;
  overflow: hidden;
  background: var(--thumb);
  border: 1px solid var(--line-soft);
  border-radius: var(--radius-sm);
}

.thumb-count {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
  height: 100%;
  color: var(--accent-ink);
}

.thumb-count svg {
  width: 20px;
  height: 20px;
}

.thumb-count b {
  font-size: 13px;
}

.body {
  flex: 1;
  min-width: 0;
}

.ctitle {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sub {
  margin: 4px 0 0;
  font-size: 11.5px;
  color: var(--faint);
}

.num {
  margin: 6px 0 0;
  font-size: 11.5px;
  color: var(--faint);
}

.pick {
  position: absolute;
  top: 10px;
  right: 10px;
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  color: var(--muted);
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: 50%;
  cursor: pointer;
}

.pick svg {
  width: 15px;
  height: 15px;
}

.collection.on .pick {
  color: #fff;
  background: var(--accent);
  border-color: var(--accent);
}

.foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 14px;
}

.count {
  font-size: 12.5px;
  color: var(--muted);
}

.primary {
  padding: 8px 18px;
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

.ghost {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 12px;
  font-size: 12.5px;
  color: var(--text);
  background: var(--field);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
}

.ghost:hover:not(:disabled) {
  border-color: var(--accent-line);
  background: var(--raised);
}

.empty {
  display: flex;
  align-items: center;
  gap: 18px;
  margin-top: 16px;
  padding: 30px 26px;
  background: var(--raised);
  border: 1px dashed var(--line);
  border-radius: var(--radius);
}

.empty.tall {
  justify-content: center;
  min-height: 320px;
  margin-top: 0;
}

.empty-icon {
  flex: none;
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  padding: 11px;
  color: var(--accent-ink);
  background: var(--accent-soft);
  border: 1px solid var(--accent-line);
  border-radius: 50%;
  animation: lib-float 3.2s ease-in-out infinite;
}

@keyframes lib-float {
  50% {
    transform: translateY(-4px);
  }
}

.empty-text {
  flex: 1;
  min-width: 0;
}

.empty-text .title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}

.empty-text .hint {
  max-width: 62ch;
  margin: 6px 0 0;
  font-size: 12.5px;
  color: var(--muted);
  line-height: 1.7;
}
</style>

<style scoped>
/* 勾选角标（+ → ✓）切换时轻弹 */
.collection.on .pick {
  animation: pick-pop 260ms var(--ease-out-expo);
}

@keyframes pick-pop {
  0% { transform: scale(0.8) rotate(-6deg); opacity: 0.4; }
  60% { transform: scale(1.08) rotate(2deg); opacity: 1; }
  100% { transform: none; opacity: 1; }
}
</style>
