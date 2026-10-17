<script setup>
// 账号面板：只在已登录时打开——显示昵称、头像与退出登录。
// 登录流程本身是**独立窗口里的官方登录页**（见后端 web_login_* 命令），
// 这里不套任何自定义登录表单。
import { computed, onUnmounted } from "vue";
import * as api from "../api";
import Icon from "./Icon.vue";

const props = defineProps({
  login: { type: Object, default: () => ({ logged_in: false }) },
});
const emit = defineEmits(["close", "logout"]);

let alive = true;
onUnmounted(() => {
  alive = false;
});

async function closeAll() {
  try {
    await api.webLoginClose();
  } catch {}
  emit("close");
}

const idText = computed(() => {
  const id = props.login.mid || "";
  if (!id) return "";
  return id.length > 16 ? `${id.slice(0, 8)}…${id.slice(-4)}` : id;
});
</script>

<template>
  <div class="backdrop" @click.self="closeAll">
    <div class="dialog pop-in" role="dialog" aria-modal="true">
      <header class="head">
        <h2>{{ login.logged_in ? "账号" : "登录微信公众平台" }}</h2>
        <button class="close" title="关闭" @click="closeAll">✕</button>
      </header>

      <template v-if="login.logged_in">
        <div class="account">
          <img v-if="login.face" class="avatar" :src="login.face" referrerpolicy="no-referrer" alt="" />
          <Icon v-else name="user" class="avatar-fallback" />
          <p class="uname">{{ login.uname || "已登录的公众账号" }}</p>
          <p v-if="idText" class="sub"><span class="num">ID {{ idText }}</span></p>
        </div>
        <p class="tip">
          公众平台登录用于「公众号全部文章」枚举（UP主式全量下载，下一版本提供）；文章与合集下载无需登录。
        </p>
        <div class="actions">
          <button class="ghost danger" @click="emit('logout')">退出登录</button>
          <button class="primary" @click="closeAll">完成</button>
        </div>
      </template>

    </div>
  </div>
</template>

<style scoped>
.backdrop {
  position: fixed;
  inset: 0;
  display: grid;
  place-items: center;
  background: var(--shade);
  z-index: 20;
}

.dialog {
  width: 356px;
  padding: 20px;
  background: var(--card);
  border-radius: 16px;
  box-shadow: 0 18px 48px rgba(66, 24, 32, 0.18);
}

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 14px;
}

h2 {
  margin: 0;
  font-size: 15px;
  font-weight: 700;
}

.close {
  width: 30px;
  height: 30px;
  font-size: 12px;
  color: var(--faint);
  border-radius: var(--radius-sm);
}

.close:hover {
  background: var(--raised);
  color: var(--text);
}

.web-login {
  padding: 16px 14px;
  text-align: center;
  background: var(--raised);
  border-radius: var(--radius);
}

.big-icon {
  width: 34px;
  height: 34px;
  color: var(--accent);
}

.hint {
  margin: 10px 0 0;
  font-size: 12.5px;
  line-height: 1.7;
  text-align: left;
  color: var(--muted);
}

.hint.dim {
  color: var(--faint);
  font-size: 11.5px;
}

.status {
  margin: 10px 0 0;
  padding: 8px 10px;
  font-size: 11.5px;
  line-height: 1.6;
  text-align: left;
  color: var(--accent-ink);
  background: var(--accent-soft);
  border-radius: var(--radius-sm);
}

.waiting {
  display: flex;
  align-items: center;
  gap: 7px;
  margin: 9px 0 0;
  font-size: 11.5px;
  color: var(--muted);
}

.dot-pulse {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--accent);
  animation: dot-pulse 1.4s var(--ease-out) infinite;
}

@keyframes dot-pulse {
  50% {
    opacity: 0.3;
    transform: scale(0.85);
  }
}

.account {
  padding: 16px;
  text-align: center;
  background: var(--raised);
  border-radius: var(--radius);
}

.avatar {
  width: 56px;
  height: 56px;
  object-fit: cover;
  background: var(--thumb);
  border: 1px solid var(--line);
  border-radius: 50%;
}

.avatar-fallback {
  width: 40px;
  height: 40px;
  color: var(--accent-ink);
}

.uname {
  margin: 10px 0 0;
  font-size: 16px;
  font-weight: 700;
}

.sub {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 9px;
  margin: 6px 0 0;
  font-size: 12px;
  color: var(--muted);
  word-break: break-all;
}

.tip {
  margin: 12px 0 0;
  font-size: 11.5px;
  line-height: 1.65;
  color: var(--faint);
  text-align: center;
}

.actions {
  display: flex;
  gap: 10px;
  margin-top: 16px;
}

.actions button {
  flex: 1;
}

.primary {
  padding: 9px;
  font-weight: 600;
  color: #fff;
  background: var(--accent);
  border-radius: var(--radius-sm);
}

.primary:hover:not(:disabled) {
  background: var(--accent-dark);
}

.primary:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.ghost {
  padding: 9px 14px;
  font-size: 12.5px;
  color: var(--text);
  background: var(--field);
  border: 1px solid var(--line);
  border-radius: var(--radius-sm);
}

.ghost:hover:not(:disabled) {
  border-color: var(--line);
  background: var(--raised);
}

.ghost:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.ghost.danger:hover {
  color: var(--err);
  border-color: var(--fail-line);
  background: var(--fail-bg);
}
</style>
