<script setup>
import { computed, ref, watch } from "vue";
import { animate } from "animejs";
import Icon from "./Icon.vue";

const props = defineProps({
  current: { type: String, required: true },
  login: { type: Object, required: true },
  queue: { type: Object, default: () => ({ active: 0, speed: 0 }) },
});
const emit = defineEmits(["navigate"]);

/** 图标模式（窄栏）：纯 UI 偏好，记在 localStorage，不动 settings.json */
const mini = ref(localStorage.getItem("wxdown.sidebar-mini") === "1");
function toggleMini() {
  mini.value = !mini.value;
  localStorage.setItem("wxdown.sidebar-mini", mini.value ? "1" : "0");
}

const items = [
  {
    key: "parse",
    icon: "link",
    label: "解析",
    hint: "添加与选择",
  },
  {
    key: "library",
    icon: "books",
    label: "内容库",
    hint: "下载记录",
  },
  {
    key: "transfer",
    icon: "transfer",
    label: "传输",
    hint: "队列与恢复",
  },
  {
    key: "settings",
    icon: "slidersV",
    label: "设置",
    hint: "偏好与维护",
  },
  {
    key: "about",
    icon: "info",
    label: "关于",
    hint: "版本与链接",
  },
];

const statusText = computed(() => {
  if (props.queue.active > 0) {
    return `下载中 ${props.queue.active} 个任务`;
  }
  return "队列空闲";
});

/** 速度数字滚动：speed 变化时用 anime 把显示值补间过去（不是跳变） */
const shownSpeed = ref(props.queue.speed || 0);
watch(
  () => props.queue.speed,
  (to) => {
    const target = to || 0;
    if (matchMedia("(prefers-reduced-motion: reduce)").matches || target === shownSpeed.value) {
      shownSpeed.value = target;
      return;
    }
    const obj = { v: shownSpeed.value };
    animate(obj, {
      v: target,
      duration: 620,
      ease: "out(3)",
      onUpdate: () => {
        shownSpeed.value = obj.v;
      },
    });
  },
);

const speedText = computed(() => {
  const speed = shownSpeed.value;
  if (!speed) return "0 B/s";
  const units = ["B", "KB", "MB", "GB"];
  let value = speed;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit > 1 ? 1 : 0)} ${units[unit]}/s`;
});
</script>

<template>
  <aside class="sidebar" :class="{ mini }">
    <nav>
      <button
        v-for="item in items"
        :key="item.key"
        class="nav-item"
        :class="{ active: current === item.key }"
        :title="mini ? `${item.label} · ${item.hint}` : undefined"
        @click="emit('navigate', item.key)"
      >
        <Icon :name="item.icon" class="icon" />
        <span v-if="!mini" class="text">
          <span class="label">{{ item.label }}</span>
          <span class="hint">{{ item.hint }}</span>
        </span>
      </button>
    </nav>

    <div class="status" :title="mini ? `${statusText} · ${speedText}` : undefined">
      <template v-if="!mini">
        <p class="line">
          <span class="dot" :class="{ busy: queue.active > 0 }"></span>
          {{ statusText }}
        </p>
        <p class="line sub num">
          {{ speedText }} · {{ login.logged_in ? "已登录" : "未登录" }}
        </p>
      </template>
      <span v-else class="dot big" :class="{ busy: queue.active > 0 }"></span>
    </div>

    <button class="mini-toggle" :title="mini ? '展开侧栏' : '收成图标栏'" @click="toggleMini">
      <Icon :name="mini ? 'chevronRight' : 'chevronLeft'" />
    </button>
  </aside>
</template>

<style scoped>
.sidebar {
  flex: none;
  width: 196px;
  display: flex;
  flex-direction: column;
  padding: 14px 12px 12px;
  background: var(--side-bg);
  border-right: 1px solid var(--line);
  /* 宽栏↔图标栏切换：宽度平滑变形（内容用 overflow 裁掉换行瞬间）。
     width 过渡在全局样式里用 body .sidebar.sidebar 提级声明——
     主题过渡规则（html.ready .sidebar）特异度更高，会盖掉这里的简写。 */
  overflow: hidden;
}

nav {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.nav-item {
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

.nav-item:hover {
  background: var(--hover);
  color: var(--text);
}

.nav-item.active {
  color: var(--accent-dark);
  background: var(--card);
  border-color: var(--accent-line);
  box-shadow: 0 1px 2px rgba(200, 130, 50, 0.1);
}

.icon {
  flex: none;
  width: 22px;
  height: 22px;
  transition: transform var(--motion-fast) var(--ease-out);
}

/* 悬停时图标朝右轻挪一点：跟文字在一起时的"指向"感 */
.nav-item:hover .icon {
  transform: translateX(1.5px);
}

.nav-item.active .icon {
  color: var(--accent);
}

.text {
  display: flex;
  flex-direction: column;
  line-height: 1.3;
}

.label {
  font-size: 13.5px;
  font-weight: 600;
}

.hint {
  font-size: 11px;
  color: var(--faint);
}

.nav-item.active .hint {
  color: var(--accent);
  opacity: 0.75;
}

.status {
  padding: 11px 11px 4px;
  border-top: 1px solid var(--line);
}

.line {
  display: flex;
  align-items: center;
  gap: 7px;
  margin: 0;
  font-size: 12px;
  color: var(--text);
}

.line.sub {
  margin-top: 3px;
  padding-left: 14px;
  font-size: 11.5px;
  color: var(--faint);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--ok);
}

.dot.busy {
  background: var(--accent);
  animation: dot-busy-pop 420ms var(--ease-out-expo);
}

@keyframes dot-busy-pop {
  0% { transform: scale(0.6); }
  60% { transform: scale(1.25); }
  100% { transform: none; }
}

/* ── 图标模式（窄栏）：只留图标，悬停靠 title 提示 ── */
.sidebar.mini {
  width: 64px;
  padding: 14px 10px 12px;
}

.sidebar.mini .nav-item {
  justify-content: center;
  gap: 0;
  padding: 10px 0;
  border-radius: var(--radius-lg);
}

.sidebar.mini .status {
  display: flex;
  justify-content: center;
  padding: 11px 0 8px;
}

.dot.big {
  width: 10px;
  height: 10px;
}

.mini-toggle {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  margin: 8px auto 0;
  padding: 0;
  color: var(--faint);
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.mini-toggle:hover {
  color: var(--text);
  background: var(--hover);
}

.mini-toggle svg {
  width: 16px;
  height: 16px;
}
</style>
