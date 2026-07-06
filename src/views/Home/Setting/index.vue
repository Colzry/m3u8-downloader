<script setup>
import PageHeader from "@/views/Home/components/PageHeader.vue";
import MainWrapper from "@/views/Home/components/MainWrapper.vue";
import { useSettingStore } from "@/store/SettingStore.js";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { appLogDir } from "@tauri-apps/api/path";
import {
    HelpCircleOutline,
    CloudDownloadOutline,
    CheckmarkCircleOutline,
    CloseCircleOutline,
    DownloadOutline,
    RefreshOutline,
} from "@vicons/ionicons5";
import { ref, shallowRef, computed } from "vue";
import { marked } from "marked";

// 引入官方的 updater 和 process 插件 API
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { createMockUpdate } from "@/utils/mockUpdate.js";

const version = import.meta.env.VITE_APP_VERSION;
const settingStore = useSettingStore();

const selectFolder = async () => {
    const selectDirectory = await open({
        directory: true,
        multiple: false,
        title: "选择下载目录",
    });
    if (selectDirectory) {
        settingStore.downloadPath = selectDirectory;
    }
};

const LOG_LEVEL_OPTIONS = [
    { label: "Trace", value: "Trace" },
    { label: "Debug", value: "Debug" },
    { label: "Info", value: "Info" },
    { label: "Warn", value: "Warn" },
    { label: "Error", value: "Error" },
    { label: "Off", value: "Off" },
];

const openAppLogDirectory = async () => {
    try {
        const logDirPath = await appLogDir();
        await openPath(logDirPath);
    } catch (e) {
        console.error("无法打开日志目录:", e);
    }
};

// 从 GitHub API 获取指定版本的发布信息
const fetchReleaseInfo = async (ver) => {
    try {
        const resp = await fetch(
            `https://api.github.com/repos/Colzry/m3u8-downloader/releases/tags/v${ver}`,
        );
        if (!resp.ok) return null;
        const data = await resp.json();
        return {
            body: data.body || "",
            date: data.published_at ? data.published_at.split("T")[0] : "",
        };
    } catch {
        return null;
    }
};

const updateModalVisible = ref(false);
const updateProgress = ref(0);
const updateModalStatus = ref("idle"); // "idle" | "checking" | "confirm" | "downloading" | "ready" | "failed" | "latest"

// 用于保存检查到的更新对象（shallowRef 避免 Proxy 破坏 Tauri Resource 私有字段）
const currentUpdate = shallowRef(null);
// 当前版本的发布信息（latest 状态下展示）
const currentVersionInfo = ref(null);
// 用于取消下载
const downloadCancelled = ref(false);
// 用于取消检查更新
let checkAbortController = null;

// Markdown 渲染
const renderedConfirmBody = computed(() => {
    const body = currentUpdate.value?.body;
    return body ? marked.parse(body) : "";
});
const renderedLatestBody = computed(() => {
    const body = currentVersionInfo.value?.body;
    return body ? marked.parse(body) : "";
});

// 点击按钮触发检查更新，Ctrl+点击可触发模拟更新（开发环境测试）
const onCheckUpdateClick = async (event) => {
    updateModalVisible.value = true;
    updateProgress.value = 0;
    updateModalStatus.value = "checking";
    currentUpdate.value = null;
    currentVersionInfo.value = null;

    // 创建新的 AbortController
    checkAbortController = new AbortController();
    const signal = checkAbortController.signal;

    try {
        // Ctrl+点击：使用模拟更新（仅开发环境）
        const update =
            import.meta.env.DEV && event.ctrlKey
                ? createMockUpdate()
                : await check();

        // 检查是否已取消
        if (signal.aborted) return;

        if (update) {
            // 发现更新，进入等待用户确认状态
            currentUpdate.value = update;
            updateModalStatus.value = "confirm";
        } else {
            // 没有更新，获取当前版本的发布信息
            currentVersionInfo.value = await fetchReleaseInfo(version);
            // 再次检查是否已取消
            if (signal.aborted) return;
            updateModalStatus.value = "latest";
        }
    } catch (e) {
        if (signal.aborted) return;
        currentVersionInfo.value = { error: String(e) };
        updateModalStatus.value = "failed";
    }
};

// 取消检查更新
const cancelCheckUpdate = () => {
    checkAbortController?.abort();
    updateModalVisible.value = false;
};

// 用户确认下载更新
const confirmUpdate = async () => {
    if (!currentUpdate.value) return;

    downloadCancelled.value = false;
    updateModalStatus.value = "downloading";
    updateProgress.value = 0;

    let downloaded = 0;
    let contentLength = 0;

    try {
        // 执行下载并安装，监听进度
        await currentUpdate.value.downloadAndInstall((event) => {
            // 每个进度回调都检查是否已取消
            if (downloadCancelled.value) return;

            switch (event.event) {
                case "Started":
                    contentLength = event.data.contentLength;
                    break;
                case "Progress":
                    downloaded += event.data.chunkLength;
                    if (contentLength > 0) {
                        updateProgress.value = Math.round(
                            (downloaded / contentLength) * 100,
                        );
                    }
                    break;
                case "Finished":
                    updateProgress.value = 100;
                    break;
            }
        });

        // 如果已被取消，不进入 ready 状态
        if (downloadCancelled.value) return;

        // 下载安装完成，等待用户决定是否重启
        updateModalStatus.value = "ready";
    } catch (e) {
        // 如果是用户主动取消，不显示错误
        if (downloadCancelled.value) return;

        currentVersionInfo.value = { error: String(e) };
        updateModalStatus.value = "failed";
    }
};

// 取消下载更新
const cancelUpdateDownload = async () => {
    downloadCancelled.value = true;
    try {
        await currentUpdate.value?.close();
    } catch (_) {
        // 忽略关闭错误
    }
    updateModalVisible.value = false;
};

// 用户选择立即重启
const restartApp = async () => {
    await relaunch();
};
</script>

<template>
    <page-header title="软件设置" />

    <main-wrapper>
        <div class="base-setting set-wrap">
            <div class="b-title title">基本设置</div>
            <div class="set-items-wrap">
                <div class="set-item">
                    <div class="set-label">
                        <div class="select-dir" @click="selectFolder">
                            选择下载文件夹
                        </div>
                    </div>
                    <div class="set-value">
                        <n-input
                            type="text"
                            size="small"
                            style="max-width: 350px"
                            :value="settingStore.downloadPath"
                            :disabled="true"
                        />
                    </div>
                </div>

                <div class="set-item">
                    <div class="set-label">
                        <div>删除已下载同时删除原文件</div>
                    </div>
                    <div class="set-value">
                        <n-switch
                            size="small"
                            v-model:value="settingStore.isDeleteDownloadFile"
                        />
                    </div>
                </div>

                <div class="set-item">
                    <div class="set-label">
                        <div>下载完成时弹出通知</div>
                    </div>
                    <div class="set-value">
                        <n-switch
                            size="small"
                            v-model:value="settingStore.enableNotification"
                        />
                    </div>
                </div>

                <div class="set-item">
                    <div class="set-label">
                        <div>关闭主窗口</div>
                    </div>
                    <div class="set-value">
                        <n-radio-group
                            v-model:value="settingStore.minimizeOnClose"
                            name="closeTheWindow"
                        >
                            <n-space>
                                <n-radio :value="false">退出程序</n-radio>
                                <n-radio :value="true">最小化</n-radio>
                            </n-space>
                        </n-radio-group>
                    </div>
                </div>
            </div>
        </div>

        <div class="download-setting set-wrap">
            <div class="d-title title">下载设置</div>
            <div class="set-items-wrap">
                <div class="set-item">
                    <div class="set-label">最大同时下载数</div>
                    <div class="set-value">
                        <n-input-number
                            size="small"
                            style="max-width: 100px"
                            v-model:value="settingStore.downloadCount"
                            placeholder="下载数"
                            :min="1"
                            :max="settingStore.physicalCores * 2"
                        />
                    </div>
                </div>
                <div class="set-item">
                    <div class="set-label">单个下载线程数</div>
                    <div class="set-value">
                        <n-input-number
                            size="small"
                            style="max-width: 100px"
                            v-model:value="settingStore.threadCount"
                            placeholder="线程数"
                            :min="1"
                            :max="settingStore.logicalCores * 8"
                        />
                    </div>
                </div>
            </div>
        </div>

        <div class="version set-wrap">
            <div class="o-title title">版本</div>
            <div class="set-items-wrap">
                <div class="set-item">
                    <div class="set-label">当前版本</div>
                    <div class="set-value">
                        <div class="version">{{ version }}</div>
                        <div class="check-update" style="margin-left: 10px">
                            <n-button
                                ghost
                                size="small"
                                @click="onCheckUpdateClick"
                            >
                                检查更新
                            </n-button>
                        </div>
                        <n-tooltip trigger="hover">
                            <template #trigger>
                                <n-icon
                                    size="1.2rem"
                                    style="cursor: pointer; margin-left: 5px"
                                >
                                    <HelpCircleOutline />
                                </n-icon>
                            </template>
                            <span>若更新失败可点击下面发布地址去下载安装</span>
                        </n-tooltip>
                    </div>
                </div>
                <div class="set-item">
                    <div class="set-label">发布地址</div>
                    <div
                        class="set-value url"
                        @click="
                            openUrl(
                                'https://github.com/Colzry/m3u8-downloader/releases',
                            )
                        "
                    >
                        https://github.com/Colzry/m3u8-downloader/releases
                    </div>
                </div>
            </div>
        </div>

        <div class="other-setting set-wrap">
            <div class="o-title title">其他</div>
            <div class="set-items-wrap">
                <div class="set-item">
                    <div class="set-label">
                        <div class="select-dir" @click="openAppLogDirectory">
                            打开日志目录
                        </div>
                    </div>
                    <div class="set-value">
                        <div style="margin-right: 5px; font: 1rem weight">
                            日志级别
                        </div>
                        <n-select
                            size="small"
                            style="max-width: 100px; margin-left: 5px"
                            v-model:value="settingStore.logLevel"
                            :options="LOG_LEVEL_OPTIONS"
                            placeholder="日志级别"
                        />
                        <n-tooltip trigger="hover">
                            <template #trigger>
                                <n-icon
                                    size="1.2rem"
                                    style="cursor: pointer; margin-left: 5px"
                                >
                                    <HelpCircleOutline />
                                </n-icon>
                            </template>
                            <span>该设置需要重启程序后生效</span>
                        </n-tooltip>
                    </div>
                </div>
            </div>
        </div>
    </main-wrapper>

    <n-modal
        v-model:show="updateModalVisible"
        :show-header="false"
        :mask-closable="
            updateModalStatus !== 'checking' &&
            updateModalStatus !== 'downloading'
        "
        :closable="false"
        :show-footer="false"
        :style="{
            width: '520px',
            borderRadius: '12px',
            overflow: 'hidden',
        }"
        :mask-style="{ backgroundColor: 'rgba(0,0,0,0.4)' }"
    >
        <div class="update-container">
            <!-- 头部 -->
            <div class="update-header">
                <n-icon :size="28" color="#fff">
                    <CloudDownloadOutline />
                </n-icon>
                <span class="update-header-title">检查更新</span>
            </div>

            <!-- 内容区 -->
            <div class="update-body">
                <!-- 检查中 -->
                <div
                    v-if="updateModalStatus === 'checking'"
                    class="update-center"
                >
                    <n-spin size="40" />
                    <p class="update-tip">正在检查更新，请稍候...</p>
                    <n-button size="small" @click="cancelCheckUpdate"
                        >取消</n-button
                    >
                </div>

                <!-- 发现新版本 -->
                <template
                    v-if="updateModalStatus === 'confirm' && currentUpdate"
                >
                    <div class="update-version-row">
                        <div class="version-badge old">
                            v{{ currentUpdate.currentVersion }}
                        </div>
                        <n-icon :size="18" color="#999" style="margin: 0 8px">
                            <RefreshOutline />
                        </n-icon>
                        <div class="version-badge new">
                            v{{ currentUpdate.version }}
                        </div>
                    </div>
                    <div v-if="currentUpdate.date" class="update-date">
                        发布于 {{ currentUpdate.date }}
                    </div>
                    <div
                        v-if="currentUpdate.body"
                        class="release-body"
                        v-html="renderedConfirmBody"
                    ></div>
                    <div class="update-actions">
                        <n-button @click="updateModalVisible = false"
                            >稍后再说</n-button
                        >
                        <n-button type="primary" @click="confirmUpdate">
                            <template #icon>
                                <n-icon><DownloadOutline /></n-icon>
                            </template>
                            立即更新
                        </n-button>
                    </div>
                </template>

                <!-- 已是最新版本 -->
                <template v-if="updateModalStatus === 'latest'">
                    <div class="update-center">
                        <n-icon :size="48" color="#18a058">
                            <CheckmarkCircleOutline />
                        </n-icon>
                        <p class="update-success-title">当前已是最新版本</p>
                        <p class="update-success-sub">v{{ version }}</p>
                    </div>
                    <div
                        v-if="currentVersionInfo?.date"
                        class="update-date"
                        style="text-align: center; margin-top: 4px"
                    >
                        发布于 {{ currentVersionInfo.date }}
                    </div>
                    <div
                        v-if="currentVersionInfo?.body"
                        class="release-body"
                        v-html="renderedLatestBody"
                    ></div>
                    <div class="update-actions">
                        <n-button
                            type="primary"
                            @click="updateModalVisible = false"
                            >关闭</n-button
                        >
                    </div>
                </template>

                <!-- 下载中 -->
                <template v-if="updateModalStatus === 'downloading'">
                    <div class="update-center">
                        <p class="update-tip">
                            正在下载更新 v{{ currentUpdate?.version }}...
                        </p>
                        <n-progress
                            :percentage="updateProgress"
                            :show-indicator="true"
                            type="line"
                            processing
                            style="width: 100%; margin: 12px 0"
                        />
                        <n-button size="small" @click="cancelUpdateDownload"
                            >取消下载</n-button
                        >
                    </div>
                </template>

                <!-- 下载完成 -->
                <template v-if="updateModalStatus === 'ready'">
                    <div class="update-center">
                        <n-icon :size="48" color="#18a058">
                            <CheckmarkCircleOutline />
                        </n-icon>
                        <p class="update-success-title">更新已准备就绪</p>
                        <p class="update-success-sub">重启应用即可完成更新</p>
                    </div>
                    <div class="update-actions">
                        <n-button @click="updateModalVisible = false"
                            >稍后重启</n-button
                        >
                        <n-button type="primary" @click="restartApp">
                            <template #icon>
                                <n-icon><RefreshOutline /></n-icon>
                            </template>
                            立即重启
                        </n-button>
                    </div>
                </template>

                <!-- 失败 -->
                <template v-if="updateModalStatus === 'failed'">
                    <div class="update-center">
                        <n-icon :size="48" color="#d03050">
                            <CloseCircleOutline />
                        </n-icon>
                        <p class="update-error-title">检查更新失败</p>
                        <p
                            v-if="currentVersionInfo?.error"
                            class="update-error-detail"
                        >
                            {{ currentVersionInfo.error }}
                        </p>
                    </div>
                    <div class="update-actions">
                        <n-button @click="updateModalVisible = false"
                            >关闭</n-button
                        >
                        <n-button
                            type="primary"
                            @click="onCheckUpdateClick($event)"
                            >重试</n-button
                        >
                    </div>
                </template>
            </div>
        </div>
    </n-modal>
</template>

<style scoped lang="less">
/* 原有样式保持不变 */
.set-wrap {
    width: 100%;
    padding: 10px;
    font-size: 0.9rem;
    border-radius: 5px;
    background-color: #fff;
    &:not(:last-child) {
        margin-bottom: 1rem;
    }
    .title {
        position: relative;
        padding-left: 10px;
        line-height: 1.1rem;
        &::before {
            content: "";
            position: absolute;
            left: 0;
            top: 0;
            width: 3px;
            height: 100%;
            background-color: #1ba059;
        }
    }

    .set-items-wrap {
        .set-item {
            margin-top: 20px;
            display: flex;
            align-items: center;
            .set-label {
                margin-left: 10px;
                flex: 3 1 0;
                color: #1f1f1f;
                .select-dir {
                    display: inline-block;
                    padding: 8px;
                    border: 1px solid #e2e2e2;
                    cursor: pointer;
                    border-radius: 5px;
                    transition: all 0.4s;
                    &:hover {
                        color: #18a058;
                        border-color: #18a058;
                    }
                }
            }
            .set-value {
                display: flex;
                align-items: center;
                flex: 7 1 0;
            }
            .url {
                cursor: pointer;
                transition: all 0.2s;
                &:hover {
                    color: #18a058;
                    text-decoration: underline;
                }
            }
        }
    }
}

.release-body {
    max-height: 240px;
    overflow-y: auto;
    padding: 12px 16px;
    background: #f8f8f8;
    border-radius: 8px;
    font-size: 0.85rem;
    color: #444;
    line-height: 1.7;
    margin-top: 12px;

    :deep(h1),
    :deep(h2),
    :deep(h3),
    :deep(h4) {
        margin: 12px 0 6px;
        font-weight: 600;
        color: #333;
    }
    :deep(h1) {
        font-size: 1.15rem;
    }
    :deep(h2) {
        font-size: 1.05rem;
    }
    :deep(h3) {
        font-size: 0.95rem;
    }

    :deep(p) {
        margin: 4px 0;
    }

    :deep(ul),
    :deep(ol) {
        padding-left: 1.4em;
        margin: 4px 0;
        list-style-position: outside;
    }

    :deep(ul) {
        list-style-type: disc;
    }

    :deep(ol) {
        list-style-type: decimal;
    }

    :deep(li) {
        margin: 2px 0;
        display: list-item;
    }

    :deep(code) {
        padding: 1px 5px;
        background: #e8e8e8;
        border-radius: 3px;
        font-size: 0.82rem;
    }

    :deep(pre) {
        margin: 8px 0;
        padding: 10px;
        background: #2d2d2d;
        color: #ccc;
        border-radius: 5px;
        overflow-x: auto;
        code {
            background: none;
            padding: 0;
            color: inherit;
        }
    }

    :deep(a) {
        color: #18a058;
        text-decoration: none;
        &:hover {
            text-decoration: underline;
        }
    }

    :deep(hr) {
        margin: 10px 0;
        border: none;
        border-top: 1px solid #ddd;
    }
}

// 更新弹窗样式
.update-container {
    background: #fff;
    border-radius: 12px;
    overflow: hidden;
}

.update-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 18px 24px;
    background: linear-gradient(135deg, #18a058, #1ba059);

    &-title {
        font-size: 1.1rem;
        font-weight: 600;
        color: #fff;
    }
}

.update-body {
    padding: 24px;
}

.update-center {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 16px 0 8px;
}

.update-tip {
    margin: 16px 0 12px;
    font-size: 0.95rem;
    color: #555;
}

.update-version-row {
    display: flex;
    align-items: center;
    justify-content: center;
    margin-bottom: 8px;
}

.version-badge {
    display: inline-block;
    padding: 4px 14px;
    border-radius: 20px;
    font-size: 0.9rem;
    font-weight: 600;

    &.old {
        background: #f0f0f0;
        color: #888;
    }

    &.new {
        background: #e8f5e9;
        color: #18a058;
    }
}

.update-date {
    text-align: center;
    font-size: 0.82rem;
    color: #999;
    margin-bottom: 4px;
}

.update-success-title {
    margin: 12px 0 4px;
    font-size: 1.05rem;
    font-weight: 600;
    color: #333;
}

.update-success-sub {
    font-size: 0.88rem;
    color: #888;
    margin: 0;
}

.update-error-title {
    margin: 12px 0 4px;
    font-size: 1.05rem;
    font-weight: 600;
    color: #d03050;
}

.update-error-detail {
    font-size: 0.82rem;
    color: #999;
    margin: 4px 0 0;
    max-width: 100%;
    word-break: break-all;
    text-align: center;
}

.update-actions {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 20px;
    padding-top: 16px;
    border-top: 1px solid #f0f0f0;
}
</style>
