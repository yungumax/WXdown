<script setup>
import { computed, nextTick, ref, watch } from "vue";
import { animate } from "animejs";
import TaskRow from "../components/TaskRow.vue";

const props = defineProps({
  tasks: { type: Array, required: true },
});
const emit = defineEmits(["cancel", "open", "reveal", "pause", "resume", "pauseAll", "resumeAll", "clear"]);

const RUNNING = ["queued", "downloading", "saving"];

/** 统计数字滚动：进行中/已结束随任务变化补间，而不是跳变 */
const runningCount = ref(0);
const pausedCount = ref(0);
const finishedCount = ref(0);
watch(
  () => props.tasks.map((t) => t.status).join(","),
  (statuses) => {
    const list = statuses ? statuses.split(",") : [];
    const running = list.filter((st) => RUNNING.includes(st)).length;
    const paused = list.filter((st) => st === "paused").length;
    pausedCount.value = paused;
    const finished = list.length - running;
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
      runningCount.value = running;
      finishedCount.value = finished;
      return;
    }
    const obj = { r: runningCount.value, f: finishedCount.value };
    animate(obj, {
      r: running,
      f: finished,
      duration: 540,
      ease: "out(3)",
      onUpdate: () => {
        runningCount.value = Math.round(obj.r);
        finishedCount.value = Math.round(obj.f);
      },
    });
  },
  { immediate: true }
);

// 新任务入队：那一行从上滑进来（只动新行，不重演整张列表；减少动态不演）
const seenIds = new Set();
watch(
  () => props.tasks.map((t) => t.id).join(","),
  async (ids, prev) => {
    for (const id of ids.split(",")) seenIds.add(id);
    if (matchMedia("(prefers-reduced-motion: reduce)").matches || !prev) return;
    const fresh = [...seenIds].filter((id) => !prev.split(",").includes(id));
    if (!fresh.length) return;
    await nextTick();
    for (const id of fresh) {
      const row = document.querySelector(`[data-id="${id}"]`);
      if (row) animate(row, { opacity: [0, 1], translateY: [-14, 0], duration: 420, ease: "outExpo" });
    }
  },
);
</script>

<template>
  <div>
    <section class="card">
      <header class="page-head">
        <div>
          <h1>传输</h1>
          <p class="lead">下载队列与任务状态；同时最多进行 2 个任务，其余排队等待。</p>
        </div>
        <span class="spacer"></span>
        <span class="counter num">
          {{ runningCount }} 进行中
          <template v-if="finishedCount"> · {{ finishedCount }} 已结束</template>
        </span>
        <button v-if="runningCount" class="ghost" @click="emit('pauseAll')">
          全部暂停
        </button>
        <button v-if="pausedCount" class="ghost" @click="emit('resumeAll')">
          全部开始
        </button>
        <button v-if="finishedCount" class="ghost" @click="emit('clear')">
          清除已结束
        </button>
      </header>

      <div v-if="!tasks.length" class="empty">
        <p class="empty-title">队列是空的</p>
        <p class="empty-hint">到「解析」页粘贴小红书笔记链接或用户主页，选择内容后即可开始下载。</p>
      </div>

      <ul v-else class="list">
        <TaskRow
          v-for="task in tasks"
          :key="task.id"
          :data-id="task.id"
          :task="task"
          @cancel="emit('cancel', task.id)"
          @open="emit('open', $event)"
          @reveal="emit('reveal', $event)"
          @pause="emit('pause', task.id)"
          @resume="emit('resume', task.id)"
        />
      </ul>
    </section>
  </div>
</template>

<style scoped>
.card {
  padding: 20px 22px;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: var(--radius-lg);
}

.page-head {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  margin-bottom: 18px;
}

h1 {
  margin: 0;
  font-size: 25px;
  font-weight: 700;
  letter-spacing: -0.2px;
}

.lead {
  margin: 7px 0 0;
  font-size: 13px;
  color: var(--muted);
}

.spacer {
  flex: 1;
}

.counter {
  align-self: center;
  font-size: 12px;
  color: var(--faint);
}

.ghost {
  align-self: center;
  padding: 7px 13px;
  font-size: 12.5px;
  color: var(--text);
  background: var(--field);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
}

.ghost:hover {
  border-color: var(--line);
  background: var(--raised);
}

.empty {
  padding: 52px 20px;
  text-align: center;
  border: 1px dashed var(--line);
  border-radius: var(--radius);
}

.empty-title {
  margin: 0;
  font-size: 13.5px;
  font-weight: 600;
  color: var(--muted);
}

.empty-hint {
  margin: 6px 0 0;
  font-size: 12.5px;
  color: var(--faint);
}

.list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 9px;
}
</style>

<style scoped>
/* 第六轮追加：统计数字等宽化——滚动时不左右晃 */
.counter.num {
  font-variant-numeric: tabular-nums;
}
</style>
