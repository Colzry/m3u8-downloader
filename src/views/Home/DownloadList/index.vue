<script setup>
import PageHeader from "@/views/Home/components/PageHeader.vue";
import HButton from "@/views/Home/components/HButton.vue";
import MainWrapper from "@/views/Home/components/MainWrapper.vue";
import DownloadItem from "@/views/Home/components/DownloadItem.vue";
import { useMessage } from "naive-ui";
import { openFolder } from "@/utils/fs.js";
import { ref, reactive } from "vue";

const message = useMessage();
const showModal = ref(false);
const formRef = ref(null);

const formData = reactive({
    videoUrl: "",
    videoName: "",
    downloadPath: "",
    batchText: "",
    headers: {},
});

// 处理自定义 Headers 的响应式数据
const headerEntries = ref([{ key: "", value: "" }]);
const userAgentOptions = [
    {
        label: "Chrome",
        value: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36",
    },
    {
        label: "Firefox",
        value: "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:143.0) Gecko/20100101 Firefox/143.0",
    },
    {
        label: "Safari",
        value: "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15",
    },
    {
        label: "Edge",
        value: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Edg/124.0.0.0",
    },
];
const resetHeaders = () => {
    headerEntries.value = [
        { key: "User-Agent", value: userAgentOptions[0]?.value || "" },
    ];
};

const rules = {
    videoUrl: {
        required: true,
        trigger: ["blur"],
        validator(rule, value) {
            const v = value?.trim();
            if (!v) return new Error("请输入视频m3u8链接");
            if (!/^https?:\/\//i.test(v))
                return new Error("URL必须以http或https开头");
            return true;
        },
    },
    videoName: {
        required: true,
        message: "请输入视频名称",
        trigger: "blur",
    },
};

import { useDownloadingStore } from "@/store/DownloadingStore";
import { useSettingStore } from "@/store/SettingStore.js";
const downloadingStore = useDownloadingStore();
const settingStore = useSettingStore();

// 初始化一些数据
// for (let id = 1; id < 15; id++) {
//   downloadingStore.addItem({
//     id: id,
//     title: '西瓜-演示下载' + id,
//     progress: 80,
//     speed: '999.99 KB/s',
//     status: 1,
//     url: 'https://sf1-cdn-tos.huoshanstatic.com/obj/media-fe/xgplayer_doc_video/hls/xgplayer-demo.m3u8',
//   });
// }
// 计算属性
const items = computed(() => downloadingStore.paginatedItems);
const isAllSelected = computed(
    () =>
        downloadingStore.selectedItems.length ===
            downloadingStore.items.length &&
        downloadingStore.selectedItems.length !== 0,
);
const isItemSelected = (id) => downloadingStore.selectedItems.includes(id);

const handleSelectAll = (checked) => {
    if (checked) {
        downloadingStore.selectAll();
    } else {
        downloadingStore.unselectAll();
    }
};

const handleSelectItem = (id, selected) => {
    downloadingStore.toggleItemSelection(id);
};

const handleSelectedNull = () => {
    if (downloadingStore.selectedItems.length === 0) {
        message.warning("请先选择需要操作的选项");
        return true;
    }
    return false;
};

// 删除选择的下载
const handleDeleteSelected = async () => {
    if (handleSelectedNull()) return;
    for (const id of downloadingStore.selectedItems) {
        await downloadingStore.removeItem(id);
    }
    downloadingStore.clearSelectedItems();
    message.success("删除成功");
};

// 取消选择的下载
const handleCancelSelected = async () => {
    if (handleSelectedNull()) return;
    for (const id of downloadingStore.selectedItems) {
        downloadingStore.cancelDownload(id).then(() => {});
    }
    downloadingStore.clearSelectedItems();
    message.success("已取消");
};

// 下载选中的项
const handleDownloadSelected = async () => {
    if (handleSelectedNull()) return;
    // 将下载项加入等待任务中
    for (const id of downloadingStore.selectedItems) {
        if (downloadingStore.getItemById(id)?.status === 2) continue;
        downloadingStore.updateItem(id, { status: 1 }); // 1 表示等待中
    }
    // 清除选中列表
    downloadingStore.clearSelectedItems();
    // 开始下载等待中的任务
    downloadingStore.tryStartNextDownloads().then(() => {});
    message.success("开始下载");
};

// 切换下载模式：单个下载 / 批量下载
const downloadMode = ref("single");

// 创建下载
const clickNewDownload = () => {
    downloadMode.value = "single";
    Object.keys(formData).forEach((key) => {
        delete formData[key];
    });
    formData.downloadPath = settingStore.downloadPath;
    showModal.value = true;
    resetHeaders(); // 统一重置headers
};

// 取消创建下载
const cancelAddDownloadHandle = () => {
    Object.keys(formData).forEach((key) => {
        delete formData[key];
    });
    showModal.value = false;
};

// 将下载项添加进列表
const addToListHandle = (item) => {
    const id = crypto.randomUUID();
    downloadingStore.addItem({
        id,
        title: item.videoName.trim(),
        progress: 0,
        status: 10,
        url: item.videoUrl.trim(),
        downloadPath: formData.downloadPath,
        headers: formData.headers,
    });
    return id; // 成功返回 ID
};

const join_loading = ref(false);
// 创建下载-加入列表回调
const handleAddClick = () => {
    if (!d_loading.value) {
        join_loading.value = true;
    }
    updateHeadersObject();
    if (downloadMode.value === "single") {
        try {
            formRef.value?.validate();
            addToListHandle({
                videoName: formData.videoName,
                videoUrl: formData.videoUrl,
            });
            message.success("添加成功");
            join_loading.value = false;
            showModal.value = false;
        } catch (e) {
            join_loading.value = false;
            return null; // 验证失败
        }
    } else if (downloadMode.value === "batch") {
        try {
            const items = parseBatch(formData.batchText);
            items.forEach((item) => {
                addToListHandle(item);
            });
            message.success("添加成功");
            join_loading.value = false;
            showModal.value = false;
        } catch (e) {
            join_loading.value = false;
            message.error(e.message);
        }
    }
};

const d_loading = ref(false);
// 创建下载-立即下载回调
const handleNowClick = () => {
    d_loading.value = true;
    updateHeadersObject();
    if (downloadMode.value === "single") {
        try {
            formRef.value?.validate();
            const id = addToListHandle({
                videoName: formData.videoName,
                videoUrl: formData.videoUrl,
            });
            downloadingStore.startDownload(id).then();
            message.success("开始下载");
            d_loading.value = false;
            showModal.value = false;
        } catch {
            d_loading.value = false;
            return null; // 验证失败
        }
    } else if (downloadMode.value === "batch") {
        try {
            const items = parseBatch(formData.batchText);
            items.forEach((item) => {
                const id = addToListHandle(item);
                downloadingStore.startDownload(id).then();
            });
            message.success("开始下载");
            d_loading.value = false;
            showModal.value = false;
        } catch (e) {
            d_loading.value = false;
            message.error(e.message);
        }
    }
};

// 添加 Header 输入框
const addHeader = () => {
    headerEntries.value.push({ key: "", value: "" });
};

// 移除 Header 输入框
const removeHeader = (index) => {
    if (headerEntries.value.length > 1) {
        headerEntries.value.splice(index, 1);
    } else {
        message.warning("默认配置，不允许删除");
    }
};

// 将 headerEntries 转换为 headers 对象
const updateHeadersObject = () => {
    const headers = {};
    headerEntries.value.forEach((entry) => {
        if (entry.key.trim()) {
            headers[entry.key] = entry.value;
        }
    });
    formData.headers = headers;
};

import { getCurrentWindow } from "@tauri-apps/api/window";
let unlisten = null; // 用于清理监听器

// 窗口 resize 事件回调
const handleWindowResized = async () => {
    const appWindow = getCurrentWindow();
    const isMaximized = await appWindow.isMaximized();

    if (isMaximized) {
        // 最大化时，增加分页大小以显示更多项
        downloadingStore.pagination.pageSize = 12;
    } else {
        // 还原时，恢复默认大小
        downloadingStore.pagination.pageSize = 5;
    }

    // 重置当前页
    downloadingStore.pagination.currentPage = 1;
};

import { open } from "@tauri-apps/plugin-dialog";
const selectFolder = async () => {
    const selectDirectory = await open({
        directory: true,
        multiple: false,
        title: "选择下载目录",
    });
    if (selectDirectory) {
        formData.downloadPath = selectDirectory;
    }
};

const showRawHeadersModal = ref(false);
const rawHeadersText = ref("");

const handleAddRawHeaders = () => {
    if (!rawHeadersText.value.trim()) {
        message.warning("请输入请求头内容");
        return;
    }

    try {
        const lines = rawHeadersText.value.trim().split("\n");
        let newEntries = [];

        // 遍历每一行，尝试解析 Key: Value 格式
        lines.forEach((line) => {
            const trimmedLine = line.trim();
            if (trimmedLine) {
                const parts = trimmedLine.split(":");
                if (parts.length >= 2) {
                    const key = parts[0].trim();
                    // 将剩余部分合并作为 value，因为 value 中可能包含冒号
                    const value = parts.slice(1).join(":").trim();

                    if (key && value) {
                        // 检查是否已存在同名的 header，如果存在则跳过或覆盖
                        // 这里选择覆盖已有的 User-Agent/Content-Type 等，以用户输入为准
                        const existingIndex = headerEntries.value.findIndex(
                            (entry) =>
                                entry.key.trim().toLowerCase() ===
                                key.toLowerCase(),
                        );

                        if (existingIndex !== -1) {
                            headerEntries.value[existingIndex].value = value;
                        } else {
                            newEntries.push({ key, value });
                        }
                    }
                }
            }
        });

        // 将新的或更新的 Entries 合并到 headerEntries 中
        headerEntries.value = [...headerEntries.value, ...newEntries];
        // 过滤掉重复项 (基于 key 忽略大小写)
        const keys = new Set();
        headerEntries.value = headerEntries.value.filter((item) => {
            const key = item.key.toLowerCase();
            return keys.has(key) ? false : (keys.add(key), true);
        });

        // 成功处理后关闭 Modal
        showRawHeadersModal.value = false;
        rawHeadersText.value = ""; // 清空文本区域
        message.success("请求头已批量添加/更新");
    } catch (e) {
        message.error("解析请求头失败，请检查格式");
        console.error("Error parsing raw headers:", e);
    }
};

// 解析批量内容 → 数组
const parseBatch = (batchText) => {
    if (!batchText || !batchText.trim()) {
        throw new Error("内容不能为空");
    }

    const lines = batchText
        .split("\n")
        .map((v) => v.trim())
        .filter(Boolean);

    if (lines.length === 0) {
        throw new Error("内容无效");
    }

    // 必须两行一组
    if (lines.length % 2 !== 0) {
        throw new Error("批量内容格式错误：必须一行链接，一行名称成对出现");
    }

    const items = [];
    const urlSet = new Set();

    for (let i = 0; i < lines.length; i += 2) {
        const url = lines[i];
        const name = lines[i + 1];

        // URL 校验
        if (!/^https?:\/\//i.test(url)) {
            throw new Error(`第 ${i + 1} 行不是有效的 URL:\n${url}`);
        }

        // 如果希望强制 m3u8 可以开这个判断（目前仅提示）：
        if (!url.includes(".m3u8")) {
            throw new Error(`URL 第 ${i + 1} 行不是 m3u8 文件：${url}`);
        }

        // 名称校验
        const finalName =
            name && name.trim() ? name.trim() : `未命名_${i / 2 + 1}`;

        // 重复 URL 检查
        if (urlSet.has(url)) {
            throw new Error(`重复的 URL：${url}`);
        }
        urlSet.add(url);

        items.push({
            videoUrl: url,
            videoName: finalName,
        });
    }

    return items;
};

const openRawHeadersModal = () => {
    rawHeadersText.value = ""; // 每次打开前清空
    showRawHeadersModal.value = true;
};

onMounted(async () => {
    const appWindow = getCurrentWindow();
    // 监听 resize 事件
    unlisten = await appWindow.onResized(handleWindowResized);
});

onUnmounted(() => {
    if (unlisten) {
        unlisten(); // 清理监听器
    }
});
</script>

<template>
    <page-header title="下载列表">
        <template #extra>
            <h-button
                label="下载目录"
                @click="openFolder(settingStore.downloadPath)"
            />
            <h-button label="新建下载" @click="clickNewDownload" />
        </template>
    </page-header>

    <main-wrapper>
        <div class="list-ctr">
            <div class="empty-ctr" v-if="downloadingStore.items.length === 0">
                <n-empty size="large" description="暂无数据" />
            </div>

            <div class="list-wrap" v-else>
                <div class="multi-choice-ctr">
                    <div class="check-wrap">
                        <n-checkbox
                            :checked="isAllSelected"
                            @update-checked="handleSelectAll"
                            label="全选"
                        />
                    </div>
                    <div class="opera-ctr">
                        <n-popconfirm
                            positive-text="确认"
                            negative-text="取消"
                            @positive-click="handleDeleteSelected"
                        >
                            <template #trigger>
                                <n-button size="small" type="error" ghost
                                    >删除</n-button
                                >
                            </template>
                            你确认要删除吗？
                        </n-popconfirm>
                        <n-popconfirm
                            positive-text="确认"
                            negative-text="算了"
                            @positive-click="handleCancelSelected"
                        >
                            <template #trigger>
                                <n-button size="small" type="warning" ghost
                                    >取消</n-button
                                >
                            </template>
                            你确认要取消选中的下载吗？
                        </n-popconfirm>
                        <n-popconfirm
                            positive-text="确认"
                            negative-text="取消"
                            @positive-click="handleDownloadSelected"
                        >
                            <template #trigger>
                                <n-button size="small" type="primary" ghost
                                    >下载</n-button
                                >
                            </template>
                            你确认要下载吗？
                        </n-popconfirm>
                    </div>
                </div>
                <download-item
                    v-for="item in items"
                    :key="item.id"
                    :id="item.id"
                    :title="item.title"
                    :progress="item.progress"
                    :is-merged="item.isMerged"
                    :status="item.status"
                    :url="item.url"
                    :speed="item.speed"
                    :selected="isItemSelected(item.id)"
                    @select="handleSelectItem"
                />
            </div>
            <!-- 分页控件 -->
            <n-pagination
                v-if="downloadingStore.totalPages > 1"
                v-model:page="downloadingStore.pagination.currentPage"
                :page-count="downloadingStore.totalPages"
                @update:page="downloadingStore.setCurrentPage"
                style="position: absolute; bottom: 10px; right: 15px"
            />
        </div>
    </main-wrapper>

    <n-modal
        v-model:show="showModal"
        :mask-closable="false"
        :show-icon="false"
        preset="dialog"
    >
        <template #header>
            <div>新建下载</div>
        </template>

        <!-- TAB 切换：单个下载 / 批量下载 -->
        <n-tabs v-model:value="downloadMode" type="line">
            <!-- ================== 单个下载 ================== -->
            <n-tab-pane name="single" tab="单个下载">
                <n-form
                    ref="formRef"
                    label-placement="left"
                    label-width="auto"
                    :model="formData"
                    :rules="rules"
                >
                    <n-form-item label="视频链接" path="videoUrl">
                        <n-input
                            v-model:value="formData.videoUrl"
                            placeholder="请输入视频 m3u8 链接"
                        />
                    </n-form-item>
                    <n-form-item label="视频名称" path="videoName">
                        <n-input
                            v-model:value="formData.videoName"
                            placeholder="请输入视频名称"
                        />
                    </n-form-item>
                </n-form>
            </n-tab-pane>

            <!-- ================== 批量下载 ================== -->
            <n-tab-pane name="batch" tab="批量下载">
                <n-input
                    type="textarea"
                    v-model:value="formData.batchText"
                    :rows="6"
                    style="margin-bottom: 10px"
                    placeholder="一行地址一行名称（自动配对）
示例：
https://example.com/a.m3u8
电影A
https://example.com/b.m3u8
电影B"
                />
            </n-tab-pane>
        </n-tabs>

        <!-- 下载目录选择 -->
        <div style="display: flex; align-items: center; margin-bottom: 10px">
            <div id="select-dir" @click="selectFolder">下载目录</div>
            <div style="flex: 1">
                <n-input
                    size="small"
                    placeholder="请选择下载目录"
                    v-model:value="formData.downloadPath"
                    :disabled="true"
                />
            </div>
        </div>
        
        <!-- ================== 高级设置 ================== -->
        <n-collapse>
            <n-collapse-item title="高级设置" name="advanced">
                <div>
                    <div
                        style="
                            display: flex;
                            align-items: center;
                            justify-content: space-between;
                        "
                    >
                        <p style="margin-bottom: 10px; color: #666">
                            自定义请求头
                        </p>
                        <n-button
                            @click="openRawHeadersModal"
                            size="small"
                            text
                            type="info"
                            style="margin-bottom: 10px; margin-left: 5px"
                        >
                            导入 Raw Headers
                        </n-button>
                    </div>

                    <!-- header 列表 -->
                    <div
                        v-for="(header, index) in headerEntries"
                        :key="index"
                        style="
                            margin-bottom: 10px;
                            display: flex;
                            align-items: center;
                        "
                    >
                        <n-input
                            v-model:value="header.key"
                            placeholder="Key"
                            style="width: 37%; margin-right: 3%"
                        />

                        <template
                            v-if="
                                header.key.trim().toLowerCase() === 'user-agent'
                            "
                        >
                            <n-select
                                v-model:value="header.value"
                                :options="userAgentOptions"
                                style="width: 50%"
                                placeholder="选择 User-Agent"
                                allow-input
                            />
                        </template>
                        <template v-else>
                            <n-input
                                v-model:value="header.value"
                                placeholder="Value"
                                style="width: 50%"
                            />
                        </template>

                        <n-button
                            @click="removeHeader(index)"
                            text
                            type="error"
                            style="width: 10%"
                        >
                            ×
                        </n-button>
                    </div>

                    <n-button
                        @click="addHeader"
                        size="small"
                        text
                        type="primary"
                    >
                        + 添加 Header
                    </n-button>
                </div>
            </n-collapse-item>
        </n-collapse>

        <!-- 底部按钮 -->
        <template #action>
            <n-button size="small" ghost @click="cancelAddDownloadHandle"
                >取消</n-button
            >

            <n-button
                :loading="join_loading"
                size="small"
                type="info"
                ghost
                @click="handleAddClick"
                >加入列表</n-button
            >

            <n-button
                :loading="d_loading"
                size="small"
                type="primary"
                @click="handleNowClick"
                >立即下载</n-button
            >
        </template>
    </n-modal>

    <n-modal
        v-model:show="showRawHeadersModal"
        :mask-closable="false"
        :show-icon="false"
        preset="dialog"
        style="width: 500px"
    >
        <template #header>
            <div>批量导入 Headers</div>
        </template>
        <n-input
            v-model:value="rawHeadersText"
            placeholder="可以粘贴从浏览器开发者工具复制的 Raw Headers 文本，
格式应为Key: Value，一行一个。
例如：
User-Agent: Mozilla/5.0...
Referer: https://example.com/
Host: cdn.example.com"
            type="textarea"
            :rows="10"
        />
        <template #action>
            <n-button size="small" ghost @click="showRawHeadersModal = false"
                >取消</n-button
            >
            <n-button size="small" type="primary" @click="handleAddRawHeaders"
                >确认添加</n-button
            >
        </template>
    </n-modal>
</template>

<style scoped lang="less">
.list-ctr {
    margin-top: 1rem;
    min-height: calc(100% - 1rem);
    border-radius: 5px;
    background-color: #fff;
    position: relative;

    .empty-ctr {
        position: absolute;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        display: flex;
        justify-content: center; /* 控制水平方向居中 */
        align-items: center; /* 控制垂直方向居中 */
    }

    .list-wrap {
        display: flex;
        flex-direction: column; /* 设置主轴为垂直方向 */
        align-items: center; /* 控制水平方向上的对齐 */
        width: 100%;
        .multi-choice-ctr {
            width: 95%;
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 1rem 0;

            .opera-ctr {
                display: flex;
                flex-direction: row;
                gap: 10px;
            }
        }
    }
}

#select-dir {
    display: inline-block;
    margin-right: 3%;
    padding: 2px 5px;
    border: 1px solid #e2e2e2;
    cursor: pointer;
    border-radius: 5px;
    transition: all 0.4s;
    &:hover {
        color: #18a058;
        border-color: #18a058;
    }
}
</style>
