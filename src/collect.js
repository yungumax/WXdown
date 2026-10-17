// 网页内收集的共享状态（WEB 窗口里点「开始/完成解析」产生）
//
// 网页是远程页面，没有 IPC 通道：它只把信号写进地址栏片段，
// 由后端读出来转成事件，前端在这里集中接住。
import { ref } from "vue";

/** 正在收集的条数（0 = 未在收集） */
export const collectCount = ref(0);

/** 收集完成后的整份列表：{ items, auto, stamp }，消费后置空 */
export const collected = ref(null);
