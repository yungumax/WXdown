<script setup>
import { computed, h, onBeforeUnmount, onMounted, ref } from "vue";
import Icon from "./Icon.vue";

import * as api from "../api";

const props = defineProps({
  login: { type: Object, required: true },
  version: { type: String, default: "" },
  /** 主题模式：light / dark / system */
  theme: { type: String, default: "system" },
});
const emit = defineEmits(["login", "set-theme"]);

const THEME_OPTIONS = [
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
  { value: "system", label: "跟随系统" },
];

/** 图标只写一份：按钮与菜单项共用同一组路径 */
const ICON_SHAPES = {
  light: [
    {
      tag: "circle",
      attrs: { cx: "12", cy: "12", r: "3.7", fill: "none", stroke: "currentColor", "stroke-width": "1.6" },
    },
    {
      tag: "path",
      attrs: {
        d: "M12 3.4v2M12 18.6v2M3.4 12h2M18.6 12h2M6.1 6.1 7.5 7.5M16.5 16.5 17.9 17.9M17.9 6.1 16.5 7.5M7.5 16.5 6.1 17.9",
        stroke: "currentColor",
        "stroke-width": "1.6",
        "stroke-linecap": "round",
      },
    },
  ],
  dark: [
    {
      tag: "path",
      attrs: {
        d: "M20 14.4A8.4 8.4 0 0 1 9.6 4 8.4 8.4 0 1 0 20 14.4Z",
        fill: "none",
        stroke: "currentColor",
        "stroke-width": "1.6",
        "stroke-linejoin": "round",
      },
    },
  ],
  system: [
    {
      tag: "circle",
      attrs: { cx: "12", cy: "12", r: "8.2", fill: "none", stroke: "currentColor", "stroke-width": "1.6" },
    },
    { tag: "path", attrs: { d: "M12 3.8a8.2 8.2 0 0 1 0 16.4Z", fill: "currentColor" } },
  ],
};

const ThemeIcon = {
  props: { mode: { type: String, required: true } },
  render() {
    return h(
      "svg",
      { viewBox: "0 0 24 24", "aria-hidden": "true" },
      (ICON_SHAPES[this.mode] || ICON_SHAPES.system).map((shape) =>
        h(shape.tag, shape.attrs)
      )
    );
  },
};

const currentLabel = computed(
  () => THEME_OPTIONS.find((option) => option.value === props.theme)?.label || "跟随系统"
);

// 点击展开菜单，再选一项；不再是点一下轮换
const picking = ref(false);
const picker = ref(null);

function onDocumentDown(event) {
  if (!picking.value) return;
  if (picker.value && !picker.value.contains(event.target)) picking.value = false;
}

function onKeydown(event) {
  if (event.key === "Escape") picking.value = false;
}

onMounted(() => {
  document.addEventListener("mousedown", onDocumentDown);
  document.addEventListener("keydown", onKeydown);
});

onBeforeUnmount(() => {
  document.removeEventListener("mousedown", onDocumentDown);
  document.removeEventListener("keydown", onKeydown);
});

function choose(mode) {
  picking.value = false;
  if (mode !== props.theme) emit("set-theme", mode);
}

/** 只有按住左键才当作拖动窗口；按钮区域不参与 */
function onDrag(event) {
  if (event.buttons !== 1) return;
  event.preventDefault();
  api.startWindowDrag();
}
</script>

<template>
  <header class="titlebar">
    <div class="brand" @mousedown="onDrag">
      <Icon name="download" class="mark" />
      <span class="name">WXdown</span>
      <span class="sub">
        Xiaohongshu Download Lab
        <span v-if="version" class="ver num">v{{ version }}</span>
      </span>
    </div>

    <div class="drag-fill" @mousedown="onDrag"></div>

    <div ref="picker" class="theme-picker">
      <button
        class="icon-btn"
        :class="{ on: picking }"
        :title="`主题：${currentLabel}（点击选择）`"
        aria-haspopup="menu"
        :aria-expanded="picking"
        @click="picking = !picking"
      >
        <ThemeIcon :mode="theme" />
      </button>

      <Transition name="picker">
        <div v-if="picking" class="theme-menu" role="menu">
          <button
            v-for="option in THEME_OPTIONS"
            :key="option.value"
            class="theme-item"
            :class="{ active: option.value === theme }"
            role="menuitemradio"
            :aria-checked="option.value === theme"
            @click="choose(option.value)"
          >
            <ThemeIcon :mode="option.value" />
            <span class="label">{{ option.label }}</span>
            <Icon v-if="option.value === theme" name="check" class="check" />
          </button>
        </div>
      </Transition>
    </div>

    <button class="login-chip" :class="{ on: login.logged_in }" @click="emit('login')">
      <img
        v-if="login.logged_in && login.face"
        class="avatar"
        :src="login.face"
        referrerpolicy="no-referrer"
        alt=""
      />
      <Icon v-else name="login" class="glyph" />
      <span class="label">{{ login.logged_in ? login.uname || "已登录" : "未登录" }}</span>
      <Icon name="chevronDown" class="caret" />
    </button>

    <div class="window-controls">
      <button class="ctrl" title="最小化" @click="api.minimizeWindow()">
        <Icon name="minus" />
      </button>
      <button class="ctrl" title="最大化 / 还原" @click="api.toggleMaximizeWindow()">
        <Icon name="maximize" />
      </button>
      <button class="ctrl close" title="关闭" @click="api.closeWindow()">
        <Icon name="close" />
      </button>
    </div>
  </header>
</template>

<style scoped>
.titlebar {
  flex: none;
  display: flex;
  align-items: center;
  height: 46px;
  padding-left: 14px;
  background: var(--card);
  /* 顶栏要压过主内容区：DOM 顺序在顶栏后，不给顶栏高 z 的话，
     主题菜单的下半截会被内容卡片盖住（踩过） */
  position: relative;
  z-index: 20;
  border-bottom: 1px solid var(--line);
  user-select: none;
}

.brand {
  display: flex;
  align-items: center;
  gap: 9px;
  cursor: default;
}

.mark {
  width: 26px;
  height: 26px;
}

.name {
  font-size: 14px;
  font-weight: 700;
  letter-spacing: 0.2px;
}

.sub {
  font-size: 11.5px;
  color: var(--faint);
}

.ver {
  margin-left: 2px;
}

.drag-fill {
  flex: 1;
  align-self: stretch;
}

.web-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 26px;
  padding: 0 10px;
  margin-right: 8px;
  font-size: 11.5px;
  font-weight: 600;
  letter-spacing: 0.4px;
  color: var(--muted);
  background: var(--field);
  border: 1px solid var(--line);
  border-radius: 999px;
  transition: color 0.15s ease, border-color 0.15s ease, background 0.15s ease;
}

.web-chip .dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  background: var(--line);
  transition: background 0.15s ease;
}

/* 网页窗口开着：点亮（点击=收起） */
.web-chip.on {
  color: var(--accent);
  border-color: var(--accent-line);
  background: var(--accent-soft);
}

.web-chip.on .dot {
  background: var(--accent);
}

.web-chip:hover {
  color: var(--accent);
  border-color: var(--accent-line);
  background: var(--accent-soft);
}

.login-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  padding: 0 11px;
  margin-right: 12px;
  font-size: 12.5px;
  color: var(--accent);
  background: var(--field);
  border: 1px solid var(--accent-line);
  border-radius: 999px;
  transition: background 0.15s ease;
}

.login-chip:hover {
  background: var(--accent-soft);
}

/* 主题选择：按钮 + 下拉菜单共用定位上下文 */
.theme-picker {
  position: relative;
  margin-right: 8px;
}

.icon-btn {
  display: grid;
  place-items: center;
  width: 34px;
  height: 34px;
  color: var(--muted);
  background: var(--field);
  border: 1px solid var(--line);
  border-radius: 50%;
  transition: color 0.15s ease, border-color 0.15s ease, background 0.15s ease;
}

.icon-btn svg {
  width: 19px;
  height: 19px;
}

.icon-btn:hover,
.icon-btn.on {
  color: var(--accent);
  border-color: var(--accent-line);
  background: var(--accent-soft);
}

.theme-menu {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  z-index: 15;
  min-width: 134px;
  padding: 5px;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: var(--radius);
  box-shadow: 0 10px 28px rgba(40, 16, 22, 0.24);
}

.theme-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 32px;
  padding: 0 8px;
  font-size: 12.5px;
  color: var(--text);
  text-align: left;
  border-radius: var(--radius-sm);
}

.theme-item:hover {
  background: var(--hover);
}

.theme-item.active {
  color: var(--accent);
}

.theme-item svg {
  flex: none;
  width: 18px;
  height: 18px;
}

.theme-item .label {
  flex: 1;
}

.theme-item .check {
  width: 17px;
  height: 17px;
  color: var(--accent);
}

.picker-enter-active,
.picker-leave-active {
  transition: opacity 0.12s ease, transform 0.12s ease;
}

.picker-enter-from,
.picker-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

.login-chip.on {
  color: var(--text);
  border-color: var(--line);
}

.login-chip .glyph {
  width: 18px;
  height: 18px;
}

/* 登录后头像替换登录图标：圆形小图，头像 CDN（wxcdn）必须带 no-referrer */
.login-chip .avatar {
  transition: transform var(--motion-fast) var(--ease-out);
  animation: avatar-in 420ms var(--ease-out-expo);
}

@keyframes avatar-in {
  from { transform: scale(0.6); opacity: 0; }
  to { transform: none; opacity: 1; }
}

.login-chip:hover .avatar {
  transform: scale(1.08);
}

.login-chip .avatar {
  flex: none;
  width: 20px;
  height: 20px;
  object-fit: cover;
  background: var(--thumb);
  border: 1px solid var(--line);
  border-radius: 50%;
}

.login-chip.on .glyph {
  color: var(--accent);
}

.caret {
  width: 15px;
  height: 15px;
  color: var(--faint);
}

.window-controls {
  display: flex;
  align-items: stretch;
  height: 100%;
}

.ctrl {
  width: 44px;
  display: grid;
  place-items: center;
  color: var(--muted);
}

.ctrl svg {
  width: 20px;
  height: 20px;
}

.ctrl:hover {
  background: var(--hover);
  color: var(--text);
}

.ctrl.close:hover {
  background: #e5484d;
  color: #fff;
}
</style>

<style scoped>
/* 收官动效：主题按钮图标随菜单开合旋转（与下拉箭头同一语言） */
.icon-btn.on svg {
  transform: rotate(180deg);
}

.icon-btn svg {
  transition: transform var(--motion) var(--ease-out);
}
</style>
