<script setup>
import Icon from "../components/Icon.vue";
defineProps({
  version: { type: String, default: "" },
});
</script>

<template>
  <div>
    <section class="card">
      <div class="identity">
        <Icon name="download" class="mark" />
        <div>
          <h1>WXdown</h1>
          <p class="ver num">v{{ version || "—" }}</p>
        </div>
      </div>

      <p class="desc">
        微信公众号内容下载工具，支持单篇文章与合集批量下载
        （正文 · 图片 · 视频 · 离线存档）；界面沿用 BILIdown 家族的设计体系。
      </p>

      <dl class="rows">
        <div class="row">
          <dt>技术栈</dt>
          <dd>Tauri 2 · Rust · Vue 3</dd>
        </div>
        <div class="row">
          <dt>下载能力</dt>
          <dd>图片原图 · 视频 mp4 直链（1080P / 720P）· 文案 txt · 并发下载与断点续传</dd>
        </div>
        <div class="row">
          <dt>内容来源</dt>
          <dd>公众号文章链接 · 合集链接（可多行批量）</dd>
        </div>
        <div class="row">
          <dt>界面</dt>
          <dd>与 BILIdown 同一套设计体系（平面化 · 浅色/深色主题 · 微信主题绿）</dd>
        </div>
      </dl>

      <p class="disclaimer">
        仅供个人学习与离线收藏使用，请尊重作者版权，不要用于传播或商业用途。
      </p>
    </section>
  </div>
</template>

<style scoped>
.card {
  padding: 22px;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: var(--radius-lg);
}

.identity {
  display: flex;
  align-items: center;
  gap: 13px;
}

.mark {
  width: 48px;
  height: 48px;
  color: var(--accent);
}

h1 {
  margin: 0;
  font-size: 22px;
  font-weight: 700;
  letter-spacing: -0.2px;
}

.ver {
  margin: 2px 0 0;
  font-size: 12.5px;
  color: var(--faint);
}

.desc {
  margin: 16px 0 0;
  font-size: 13px;
  line-height: 1.75;
  color: var(--muted);
  max-width: 62ch;
}

.rows {
  margin: 18px 0 0;
}

.row {
  display: flex;
  gap: 16px;
  padding: 12px 0;
  border-top: 1px solid var(--line-soft);
}

dt {
  flex: none;
  width: 86px;
  font-size: 12.5px;
  color: var(--muted);
}

dd {
  margin: 0;
  font-size: 13px;
}

.disclaimer {
  margin: 18px 0 0;
  padding: 11px 13px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--muted);
  background: var(--raised);
  border-radius: var(--radius-sm);
}
</style>
