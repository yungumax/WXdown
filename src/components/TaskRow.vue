<script setup>
import { computed, ref, watch } from "vue";
import { animate } from "animejs";

const props = defineProps({
  task: { type: Object, required: true },
});
const emit = defineEmits(["cancel", "open", "reveal", "pause", "resume"]);

const RUNNING = ["queued", "downloading", "saving"];

// 任务完成/失败：整行底色闪一次（既有 done/fail 色系的浅染，回落为透明）
const rowEl = ref(null);
watch(
  () => props.task.status,
  (now, was) => {
    if (!rowEl.value || now === was || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    if (now !== "done" && now !== "failed") return;
    animate(rowEl.value, {
      backgroundColor: [
        now === "done" ? "rgba(70, 192, 140, 0.16)" : "rgba(236, 106, 98, 0.16)",
        "rgba(0, 0, 0, 0)",
      ],
      duration: 900,
      ease: "out(2)",
    });
  }
);
const STATUS_TEXT = {
  queued: "排队中",
  downloading: "下载中",
  saving: "保存中",
  paused: "已暂停",
  done: "已完成",
  failed: "失败",
  canceled: "已取消",
};

/** 三个阶段各自的状态，驱动进度条与状态点（公众号版：图片 | 视频语音 | 正文与存档） */
const stages = computed(() => {
  const task = props.task;
  const finished = task.status === "done";
  const failed = task.status === "failed";
  const canceled = task.status === "canceled";

  const build = (pct, doneByStatus, active) => {
    if (doneByStatus || pct >= 100) return { pct: 100, text: "完成", state: "done" };
    if (active) return { pct, text: `${pct.toFixed(0)}%`, state: "active" };
    if (failed) return { pct, text: "中断", state: "failed" };
    if (canceled) return { pct, text: "已取消", state: "idle" };
    return { pct, text: "待处理", state: "idle" };
  };

  const hasImages = (task.image_count ?? 0) > 0;
  const hasVideos = (task.video_count ?? 0) > 0;

  return [
    {
      key: "image",
      label: "图片",
      ...build(
        task.image_pct,
        finished || (!hasImages && task.status !== "queued"),
        task.status === "downloading" && task.image_pct < 100
      ),
    },
    {
      key: "video",
      label: "视频",
      ...build(
        task.video_pct,
        finished || (!hasVideos && task.status !== "queued"),
        task.status === "downloading" && task.image_pct >= 100
      ),
    },
    {
      key: "save",
      label: "文案",
      ...build(finished ? 100 : 0, finished, task.status === "saving"),
    },
  ];
});

const isSaveIndeterminate = computed(() => props.task.status === "saving");

const bytesText = computed(() => {
  const task = props.task;
  if (task.status === "done") return human(task.total || task.downloaded);
  if (!task.total) return human(task.downloaded);
  return `${human(task.downloaded)} / ${human(task.total)}`;
});

// 速度只在真正跑的阶段显示：暂停/排队后 reporter 的尾帧可能带出残余速度，
// 状态守卫保证「已暂停」行不会挂着幽灵速度
const speedText = computed(() =>
  ["downloading", "saving"].includes(props.task.status) && props.task.speed_bps > 0
    ? `${human(props.task.speed_bps)}/s`
    : ""
);

const canCancel = computed(() =>
  RUNNING.includes(props.task.status) || props.task.status === "paused"
);
const canOpen = computed(() => !!props.task.output_path);

function human(bytes) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = Number(bytes) || 0;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${value} B` : `${value.toFixed(unit > 1 ? 2 : 1)} ${units[unit]}`;
}
</script>

<template>
  <li ref="rowEl" class="row" :class="task.status">
    <div class="head">
      <h4 class="title" :title="task.title">{{ task.title }}</h4>
      <span v-if="task.quality_label" class="quality">{{ task.quality_label }}</span>
      <span class="spacer"></span>
      <span class="status">{{ STATUS_TEXT[task.status] }}</span>
      <button v-if="task.status === 'downloading'" class="action" @click="emit('pause')">暂停</button>
      <button v-if="task.status === 'paused'" class="action" @click="emit('resume')">继续</button>
      <button v-if="canCancel" class="action" @click="emit('cancel')">取消</button>
      <template v-else-if="canOpen">
        <button
          class="action"
          :title="`打开所在文件夹：${task.output_path}`"
          @click="emit('reveal', task.output_path)"
        >
          {{ task.status === "done" ? "打开位置" : "定位" }}
        </button>
        <button
          v-if="task.status === 'done'"
          class="action"
          :title="`打开文件：${task.output_path}`"
          @click="emit('open', task.output_path)"
        >
          打开
        </button>
      </template>
    </div>

    <div class="bar">
      <div
        v-for="(stage, index) in stages"
        :key="stage.key"
        class="seg"
        :class="[stage.state, index === 2 && isSaveIndeterminate ? 'indeterminate' : '']"
      >
        <span
          class="fill"
          :style="
            index === 2 && isSaveIndeterminate
              ? null
              : { transform: `scaleX(${stage.pct / 100})` }
          "
        ></span>
      </div>
    </div>

    <div class="meta">
      <ul class="stages">
        <li v-for="stage in stages" :key="stage.key" :class="stage.state">
          <span class="pip"></span>
          <span class="name">{{ stage.label }}</span>
          <span class="value num">{{ stage.text }}</span>
        </li>
      </ul>
      <span class="spacer"></span>
      <span v-if="task.status === 'failed'" class="message error" :title="task.message">
        {{ task.message }}
      </span>
      <span class="bytes num">{{ bytesText }}</span>
      <span v-if="speedText" class="speed num">{{ speedText }}</span>
    </div>
  </li>
</template>

<style scoped>
.row {
  padding: 13px 15px;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: var(--radius);
}

.row.done {
  border-color: var(--done-line);
  background: var(--done-bg);
}

.row.failed {
  border-color: var(--fail-line);
  background: var(--fail-bg);
}

.head {
  display: flex;
  align-items: center;
  gap: 9px;
}

.title {
  margin: 0;
  max-width: 46%;
  font-size: 13.5px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.quality {
  flex: none;
  padding: 1px 7px;
  font-size: 11.5px;
  color: var(--muted);
  background: var(--raised);
  border: 1px solid var(--line-soft);
  border-radius: 5px;
}

.spacer {
  flex: 1;
}

.status {
  font-size: 12px;
  color: var(--muted);
}

.row.downloading .status,
.row.saving .status {
  color: var(--accent-dark);
}

.row.done .status {
  color: var(--ok);
}

.row.failed .status {
  color: var(--err);
}

.action {
  font-size: 12px;
  color: var(--muted);
  padding: 3px 10px;
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
  background: var(--field);
}

.action:hover {
  color: var(--text);
  border-color: var(--line);
  background: var(--raised);
}

/* 三段式进度：图片流 | 视频流 | 文案与收尾 */
.bar {
  display: flex;
  gap: 3px;
  height: 6px;
  margin: 11px 0 9px;
}

.seg {
  flex: 1;
  background: var(--seg-track);
  border-radius: 3px;
  overflow: hidden;
}

.fill {
  display: block;
  height: 100%;
  width: 100%;
  background: var(--accent);
  border-radius: 3px;
  /* 用 scaleX 推进而不是 width：width 动画会触发布局抖动 */
  transform: scaleX(0);
  transform-origin: left center;
}

.seg.done .fill {
  background: var(--ok);
}

.seg.idle .fill {
  background: var(--seg-idle);
}

.row.failed .seg .fill {
  background: var(--err);
  opacity: 0.55;
}

/* 进度推进要平滑：后端每几百毫秒推一次，直接跳格像卡顿 */
.seg .fill {
  transition: transform var(--motion) linear;
}

/* 保存阶段没有百分比，用流动填充表示进行中 */
.seg.indeterminate .fill {
  width: 100%;
  transform: none;
  background: linear-gradient(
    90deg,
    var(--accent-soft) 0%,
    var(--accent) 50%,
    var(--accent-soft) 100%
  );
  background-size: 200% 100%;
  animation: sweep 1.3s linear infinite;
}

@keyframes sweep {
  from {
    background-position: 100% 0;
  }
  to {
    background-position: -100% 0;
  }
}

.meta {
  display: flex;
  align-items: center;
  gap: 14px;
  font-size: 12px;
  color: var(--muted);
}

.stages {
  display: flex;
  gap: 14px;
  list-style: none;
  margin: 0;
  padding: 0;
}

.stages li {
  display: flex;
  align-items: center;
  gap: 5px;
}

.pip {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--seg-idle);
}

.stages li.active .pip {
  background: var(--accent);
  /* 正在跑的阶段：小圆点呼吸，一眼看出哪段在工作 */
  animation: pip-pulse 1.6s var(--ease-out) infinite;
}

@keyframes pip-pulse {
  50% {
    opacity: 0.35;
  }
}

.stages li.done .pip {
  background: var(--ok);
}

.stages li.failed .pip {
  background: var(--err);
}

.stages .name {
  color: var(--faint);
}

.stages .value {
  color: var(--text);
}

.stages li.idle .value {
  color: var(--faint);
}

.bytes {
  color: var(--text);
}

.message.error {
  max-width: 40%;
  color: var(--err);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>

<style scoped>
/* 收官动效：传输行悬停左缘指示线（与选择页表格同一语言；无 td 覆盖问题，直接挂行上） */
.row:hover {
  box-shadow: inset 2px 0 0 var(--accent);
}

/* 收官动效：进度阶段达标瞬间闪一下亮度（done 类落上时播） */
.seg.done .fill {
  animation: stage-flash 500ms var(--ease-out);
}

@keyframes stage-flash {
  0% { filter: brightness(1.9); }
  100% { filter: brightness(1); }
}
</style>
