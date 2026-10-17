<script setup>
import { ICONS } from "../icons.js";

// 图标集来自 iconfont 公共库 cid=54475（见 src/icons.js 的说明）。
// 尺寸由所在位置的 CSS 决定（父级作用域的 .icon / .ghost svg 之类照样命中这个根 svg），
// 颜色跟随 currentColor。
const props = defineProps({
  name: { type: String, required: true },
});

const paths = ICONS[props.name] ?? [];
</script>

<template>
  <svg viewBox="0 0 1024 1024" aria-hidden="true">
    <path
      v-for="(path, index) in paths"
      :key="index"
      :d="path.d"
      :fill-rule="path.rule || null"
      fill="currentColor"
    />
  </svg>
</template>
