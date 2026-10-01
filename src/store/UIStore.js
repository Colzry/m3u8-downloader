import { defineStore } from "pinia";

export const useUIStore = defineStore("UI", {
    state: () => ({
        collapsed: false, // collapsed 状态，控制侧边栏是否收起
        // 待用户确认的深链下载队列（非持久化，仅运行时使用）
        deepLinkQueue: [],
    }),
    actions: {
        toggleCollapsed() {
            this.collapsed = !this.collapsed; // 切换 collapsed 状态
        },
        // 追加一条待确认的深链下载
        pushDeepLink(payload) {
            this.deepLinkQueue.push(payload);
        },
        // 取出队首的深链下载（无则返回 null）
        shiftDeepLink() {
            return this.deepLinkQueue.shift() ?? null;
        },
    },
});
