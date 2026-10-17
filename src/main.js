import { createApp } from "vue";
import App from "./App.vue";
import "./styles.css";

createApp(App).mount("#app");

// 窗口以隐藏方式启动，必须等首帧真正画完再显示。
// mount() 是同步的，但浏览器还没绘制这一帧；此时 show() 会先露出一个没有内容的
// 空窗口（深色主题下就是一块空壳），等下一帧才有内容。
// 连续两次 rAF：第二个回调触发时第一帧已经提交，窗口一出现就有内容。
requestAnimationFrame(() => {
  requestAnimationFrame(() => {
    import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) => getCurrentWindow().show())
      .catch(() => {
        // 浏览器预览环境没有 Tauri 运行时，忽略
      });
  });
});
