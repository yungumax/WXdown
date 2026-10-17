// 下载请求与命名变量：解析页与内容库共用同一份实现。
//
// 命名规则是"一个条目叫什么"的唯一出处，两个入口必须算出同一个名字 ——
// 所以 batchNaming / singleNaming 只在这里实现一次，谁也别抄第二份。

import * as api from "./api";

export const KIND_LABELS = {
  article: "单篇文章",
  album: "合集",
  account: "公众号全部文章",
};

/** 图片规格选项（唯一定义，设置页与下载弹层共用）。
 * 公众号文章图片只有页面放出的一档（mmbiz data-src 原图），
 * 没有清晰度可挑——选项保留一档，文案如实说明。 */
export const IMAGE_CHOICES = [
  { value: "large", label: "原图（微信放出的最高画质）" },
];

/** 视频清晰度选项（唯一定义，引导页与设置页共用）。
 *  文章内视频（微信原生/腾讯视频）由工具尽力解析直链，按能拿到的
 *  最高画质保存，没有档位可选。 */
export const VIDEO_QUALITIES = [
  { value: "auto", label: "最佳可用（自动解析最高画质直链）" },
];

export function localDate(unixSecs) {
  const d = unixSecs ? new Date(unixSecs * 1000) : new Date();
  const pad = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** 发布年月：文件夹模板 {year}/{month} 用 */
export function yearMonth(unixSecs) {
  const d = unixSecs ? new Date(unixSecs * 1000) : new Date();
  const pad = (n) => String(n).padStart(2, "0");
  return { year: String(d.getFullYear()), month: pad(d.getMonth() + 1) };
}

/** 单条笔记的命名变量 */
export function singleNaming(probe) {
  const entry = probe.items?.[0] ?? {};
  const created = entry.created_at || probe.created_at || 0;
  const { year, month } = yearMonth(created);
  return {
    author: probe.author || entry.author || "",
    // uid 是字符串字段：这里写 `|| 0` 会把"没有 uid"变成整数 0，
    // 后端 NamingVars.uid 是 String，反序列化直接报 invalid type（实测踩过）
    uid: probe.uid || "",
    bid: probe.bid || entry.bid || "",
    mid: probe.mid || entry.mid || "",
    collection: "",
    publish_date: localDate(created),
    date: localDate(),
    year,
    month,
    source_kind: KIND_LABELS[probe.kind] ?? "文章",
    // 单条也带序号（默认模板含 {index}）：固定 1，补一位不出现"00"
    index: 1,
    index_pad: 1,
  };
}

/** 批量条目的命名变量：合集按"由旧到新"给序号（position 由表格算好传入） */
export function batchNaming(batch, entry, position) {
  const { year, month } = yearMonth(entry.created_at);
  return {
    author: entry.author || batch.author || "",
    // 同上：必须是字符串。网页收集来的来源 uid 为空，`|| 0` 会让整条下载请求被拒
    uid: batch.uid || "",
    bid: entry.bid || "",
    mid: entry.mid || "",
    // 合集名进命名变量（文件夹模板 {collection} 用）
    collection: batch.kind === "album" ? batch.title || "" : "",
    publish_date: localDate(entry.created_at),
    date: localDate(),
    year,
    month,
    source_kind: KIND_LABELS[batch.kind] ?? "文章",
    // {index} 的补零宽度按来源总数算：20 条补到 2 位、几千条补到 4 位，
    // 这样目录按名称排序才是 01、02 … 10，而不是 1、10、2。
    // 总数拿不到时用本批条数；来源以后继续拉长，已编号的位数也不会变。
    index_pad: String(Math.max(batch.total || batch.items.length, 1)).length,
    index: position,
  };
}

/** 默认视频清晰度：优先取可用档里的最佳（auto），设置里挑了具体档就用那一档 */
export function pickDefaultQuality(probe, settings) {
  const picked = settings?.video_quality && settings.video_quality !== "auto"
    ? settings.video_quality
    : "auto";
  if (picked !== "auto" && probe.qualities?.some((q) => q.value === picked && q.available)) {
    return picked;
  }
  return "auto";
}

/** 序号列的补零宽度与文件名一致：100 条补 3 位，避免表里 "01" 而磁盘上是 "001" */
export function paddedSeq(row) {
  const probe = row.source.probe;
  const width = String(Math.max(probe.total || probe.items.length, 1)).length;
  return String(row.abs).padStart(Math.max(width, 2), "0");
}
