import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { useDownloadedStore } from "@/store/DownloadedStore.js";
import { useSettingStore } from "@/store/SettingStore.js";
import { listen } from "@tauri-apps/api/event";

export const useDownloadingStore = defineStore("Downloading", {
  /**
   status 0-已取消 1-等待中 2-下载中 3-下载完成 4-合并中 5-合并完成 10-初始化或新添加 400-合并失败
   **/

  state: () => ({
    items: [], // 下载项列表
    selectedItems: [], // 选中的下载项ID列表

    // 存储所有任务的监听器 { [taskId]: [listenFn1, listenFn2] }
    taskListeners: {},

    pagination: {
      currentPage: 1,
      pageSize: 5,
      total: 0,
    },
  }),

  getters: {
    // 分页后的数据
    paginatedItems: (state) => {
      const start =
        (state.pagination.currentPage - 1) * state.pagination.pageSize;
      const end = start + state.pagination.pageSize;
      return state.items.slice(start, end);
    },

    // 总页数
    totalPages: (state) => {
      return Math.ceil(state.items.length / state.pagination.pageSize);
    },
  },

  actions: {
    // 在应用启动时调用，用于清理从持久化存储中加载的状态。
    init() {
      for (const item of this.items) {
        // 1. 将所有"下载中"或"等待中"以及"合并中"的任务重置为"初始化或新添加"
        // status 10 (初始化或新添加) 是重启后的标准"待命"状态
        if (item.status !== 10) {
          item.status = 10; // 10-初始化或新添加
        }
      }
    },

    setCurrentPage(page) {
      this.pagination.currentPage = Math.max(
        1,
        Math.min(page, this.totalPages),
      );
    },

    setPageSize(size) {
      this.pagination.pageSize = size;
      this.pagination.currentPage = 1;
      this.updatePaginationTotal();
    },

    updatePaginationTotal() {
      this.pagination.total = this.items.length;
    },

    // 删除元素后调整页码
    adjustCurrentPageAfterRemove() {
      this.updatePaginationTotal();
      if (this.paginatedItems.length === 0 && this.pagination.currentPage > 1) {
        this.setCurrentPage(this.pagination.currentPage - 1);
      }
    },

    // 添加下载项
    addItem(item) {
      this.items.push(item);
      this.updatePaginationTotal();
    },

    // 通过 ID 获取下载项
    getItemById(id) {
      return this.items.find((item) => item.id === id) || null;
    },

    // 检查最大下载数，如果达到最大并发数，设置为等待状态
    checkMaxDownloads(id) {
      const settingStore = useSettingStore();
      // 获取当前活跃任务数
      const activeCount = this.items.filter(
        (item) => item.status === 2, // 2 表示下载中
      ).length;

      // 如果达到最大并发数，设置为等待状态
      if (activeCount >= settingStore.downloadCount) {
        this.updateItem(id, { status: 1 }); // 1 表示等待中
        return true;
      }
      return false;
    },

    // 取消下载（保留临时目录，支持断点续传）
    async cancelDownload(id) {
      const item = this.getItemById(id);
      if (!item) return;

      // 清理该任务的所有事件监听器
      this.cleanupTaskListeners(id);

      // 如果任务正在下载，调用后端取消
      if (item.status === 2) {
        try {
          await invoke("cancel_download", { id });
        } catch (e) {
          console.error(`取消任务 ${id} 失败:`, e);
        }
      }

      // 更新状态为已取消
      this.updateItem(id, { status: 0 }); // 0 表示已取消

      // 如果任务之前是活跃的，尝试启动等待队列中的下一个任务
      await this.tryStartNextDownloads();
    },

    // 继续下载（使用断点续传）
    async continueDownload(id) {
      // 如果达到最大并发数，设置为等待状态
      if (this.checkMaxDownloads(id)) return;

      const item = this.getItemById(id);
      if (item) {
        // 直接调用 startDownload，后端会自动处理断点续传
        await this.startDownload(id);
      }
    },

    // 移除下载项并删除临时目录
    async removeItem(id) {
      const item = this.getItemById(id);
      if (!item) return;
      const wasActive = item?.status === 2;

      try {
        // outputDir 从 useSettingStore 中获取
        const settingStore = useSettingStore();

        // delete_download 会删除临时目录和所有下载进度
        await invoke("delete_download", {
          id: id,
          outputDir: settingStore.downloadPath,
        });
      } catch (e) {
        console.error(`删除任务 ${id} 失败:`, e);
      }

      this.cleanupTaskListeners(id);
      this.items = this.items.filter((i) => i.id !== id);
      this.selectedItems = this.selectedItems.filter((i) => i !== id);
      this.adjustCurrentPageAfterRemove();

      if (wasActive) {
        await this.tryStartNextDownloads();
      }
    },

    // 全选
    selectAll() {
      this.selectedItems = this.items.map((item) => item.id);
    },

    // 取消全选
    unselectAll() {
      this.selectedItems = [];
    },

    // 添加或移除单个选项
    toggleItemSelection(id) {
      const index = this.selectedItems.indexOf(id);
      if (index === -1) {
        this.selectedItems.push(id); // 如果未选中，则添加
      } else {
        this.selectedItems.splice(index, 1); // 如果已选中，则移除
      }
    },

    // 更新下载项
    updateItem(id, updates) {
      const item = this.items.find((item) => item.id === id);
      if (item) {
        // 将 updates 中的属性添加或覆盖到 item 中
        Object.keys(updates).forEach((key) => {
          item[key] = updates[key]; // 直接设置属性值，添加或覆盖
        });
      }
    },

    clearSelectedItems() {
      this.selectedItems = [];
    },

    // 清理指定任务的监听器
    cleanupTaskListeners(taskId) {
      if (this.taskListeners[taskId]) {
        this.taskListeners[taskId].forEach((listen) => listen?.());
        delete this.taskListeners[taskId];
      }
    },

    // 启动新下载任务
    async startDownload(taskId) {
      const settingStore = useSettingStore();
      const downloadedStore = useDownloadedStore();

      const currentItem = this.getItemById(taskId);
      if (!currentItem || currentItem.status === 2) {
        return;
      }

      // 如果达到最大并发数，设置为等待状态
      if (this.checkMaxDownloads(taskId)) return;

      // 清理已有监听器
      this.cleanupTaskListeners(taskId);

      this.updateItem(taskId, { status: 2 });

      // 创建事件监听器
      const [listenCreateDir, listenProgress, listenStartMerge, listenMerge] =
        await Promise.all([
          // 创建临时目录监听
          listen("create_temp_directory", (event) => {
            const data = event.payload;
            if (data.id === taskId && data.isCreatedTempDir) {
              // 更新状态示例（需要你实际的状态管理逻辑）
              const item = this.items.find((item) => item.id === taskId);
              if (item) {
                item.isCreatedTempDir = data.isCreatedTempDir;
              }
            }
          }),

          // 下载进度监听
          listen("download_progress", (event) => {
            const data = event.payload;
            if (data.id === taskId) {
              const currentItem = this.getItemById(data.id);
              if (currentItem && currentItem.status === 2) {
                this.updateItem(data.id, { ...data });
              }
            }
          }),

          // 开始合并监听
          listen("start_merge_video", (event) => {
            const data = event.payload;
            if (data.id === taskId) {
              this.updateItem(data.id, { status: 4 });
            }
            // 开始合并，触发队列检查，继续下载下一个
            this.tryStartNextDownloads();
          }),

          // 合并视频监听
          listen("merge_video", (event) => {
            const data = event.payload;
            if (data.id === taskId && !data.isMerged) {
              // 合并失败
              const settingStore = useSettingStore();
              if (settingStore.enableNotification) {
                const item = this.items.find((i) => i.id === taskId);
                const title = item?.title || "视频";
                invoke("send_notification_cmd", {
                  title: "合并失败",
                  body: `${title} 合并失败`,
                }).catch((e) => {
                  console.error("发送通知失败:", e);
                });
              }
              this.updateItem(data.id, { status: 400 });
              this.cleanupTaskListeners(taskId);
              this.tryStartNextDownloads();
              return;
            }
            if (data.id === taskId && data.isMerged) {
              // 发送系统通知
              const settingStore = useSettingStore();
              if (settingStore.enableNotification) {
                const item = this.items.find((i) => i.id === taskId);
                const title = item?.title || "视频";
                // 通过 Rust 后端发送系统通知
                invoke("send_notification_cmd", {
                  title: "下载完成",
                  body: `${title} 已下载完成`,
                }).catch((e) => {
                  console.error("发送通知失败:", e);
                });
              }

              // 迁移数据示例
              const item = this.items.find((i) => i.id === taskId);
              if (item) {
                Object.keys(data).forEach((key) => {
                  item[key] = data[key]; // 直接设置属性值，添加或覆盖
                });
                downloadedStore.addItem(item);
                this.items = this.items.filter((item) => item.id !== taskId);
                this.cleanupTaskListeners(taskId);
                this.adjustCurrentPageAfterRemove();
              }
            }
          }),
        ]);

      // 存储监听器
      this.taskListeners[taskId] = [
        listenCreateDir,
        listenProgress,
        listenStartMerge,
        listenMerge,
      ];

      const item = this.getItemById(taskId);
      if (item) {
        this.updateItem(item.id, { isDownloaded: true, status: 2 });
        await invoke("start_download", {
          id: item.id,
          url: item.url,
          name: item.title,
          outputDir: item.downloadPath,
          threadCount: settingStore.threadCount,
          headers: item.headers || {},
        }).catch(async (err) => {
          await this.cancelDownload(item.id);
          // 发送系统通知
          if (settingStore.enableNotification) {
            invoke("send_notification_cmd", {
              title: "下载失败",
              body: `${item.title} 下载失败`,
            }).catch((e) => {
              console.error("发送通知失败:", e);
            });
          }
          this.$notify.error({
            content: this.getItemById(item.id).title + "下载失败",
            meta: err,
            keepAliveOnHover: true,
            duration: 5000,
          });
          console.error(this.getItemById(item.id).title + "下载失败");
        });
      }
    },

    // 尝试启动等待中的任务
    async tryStartNextDownloads() {
      const settingStore = useSettingStore();

      // 计算可用槽位
      const activeCount = this.items.filter((item) => item.status === 2).length;
      const availableSlots = settingStore.downloadCount - activeCount;

      if (availableSlots <= 0) return;

      // 找到前 N 个等待中的任务
      const waitingTasks = this.items
        .filter((item) => item.status === 1)
        .slice(0, availableSlots);

      // 启动这些任务
      for (const task of waitingTasks) {
        if (task.status === 1) {
          this.startDownload(task.id);
        }
      }
    },
  },
  persist: true, // 启用持久化
});
