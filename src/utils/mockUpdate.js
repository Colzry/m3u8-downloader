/**
 * 模拟更新对象，用于开发环境测试更新流程
 * 用法：Ctrl+点击「检查更新」按钮触发
 */

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const MOCK_BODY = `## v1.3.0 更新内容

### 新功能
- 支持批量导入 m3u8 地址
- 新增下载完成后的通知提醒
- 支持自定义 FFmpeg 合并参数

### 优化
- 提升分片下载速度，优化并发策略
- 优化内存占用，减少大文件下载时的资源消耗
- 改进错误提示信息，更加友好易懂

### 修复
- 修复部分加密流无法解密的问题
- 修复网络波动时偶尔下载失败的 bug
- 修复 Windows 路径包含中文时合并失败的问题`;

export function createMockUpdate() {
  // 用于取消模拟下载
  let aborted = false;

  const update = {
    available: true,
    currentVersion: "1.2.7",
    version: "1.3.0",
    date: "2026-07-06",
    body: MOCK_BODY,
    rawJson: {},

    async downloadAndInstall(onEvent) {
      aborted = false;
      const totalSize = 25 * 1024 * 1024; // 25MB
      let downloaded = 0;

      onEvent?.({ event: "Started", data: { contentLength: totalSize } });

      const chunkSize = 512 * 1024;
      while (downloaded < totalSize) {
        if (aborted) return;
        await sleep(150);
        const chunk = Math.min(chunkSize, totalSize - downloaded);
        downloaded += chunk;
        onEvent?.({ event: "Progress", data: { chunkLength: chunk } });
      }

      if (!aborted) {
        onEvent?.({ event: "Finished" });
      }
    },

    async download(onEvent, _options) {
      await this.downloadAndInstall(onEvent);
    },

    async install() {},

    async close() {
      aborted = true;
    },
  };

  return update;
}
