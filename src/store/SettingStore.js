import { defineStore } from "pinia";

export const useSettingStore = defineStore("Setting", {
  state: () => ({
    downloadPath: "",
    downloadCount: 1, // 下载数
    threadCount: 12, // 线程数
    physicalCores: 1, // 物理核心数
    logicalCores: 4, // 逻辑核心数
    isDeleteDownloadFile: false, // 是否删除下载原文件
    enableNotification: true, // 下载完成是否弹出系统通知
    minimizeOnClose: true, // false 退出程序  true 最小化
    logLevel: "Info", // 日志级别
  }),
  actions: {},
  persist: true, // 启用持久化
});
