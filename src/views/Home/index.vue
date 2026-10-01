<script setup>
import { h, ref } from "vue";
import { NIcon } from "naive-ui";
import { RouterLink } from "vue-router";
import { CloudDownload, CloudDone, Cog } from "@vicons/ionicons5";
import router from "@/router/index.js";
import { useUIStore } from "@/store/UIStore";
const UIStore = useUIStore();
import { exists } from "@tauri-apps/plugin-fs";
import { useNotification } from "naive-ui";
import { videoDir } from "@tauri-apps/api/path";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSettingStore } from "@/store/SettingStore.js";
import { useDownloadingStore } from "@/store/DownloadingStore.js";

const downloadingStore = useDownloadingStore();
const settingStore = useSettingStore();
const notification = useNotification();
onBeforeMount(async () => {
    const videoPath = await videoDir();
    if (settingStore.downloadPath === "") {
        settingStore.downloadPath = videoPath;
        const [physicalCores, logicalCores] = await invoke("get_cpu_info");
        settingStore.physicalCores = physicalCores;
        settingStore.logicalCores = logicalCores;
    } else {
        const isExistsDownloadPath = await exists(settingStore.downloadPath);
        if (!isExistsDownloadPath) {
            settingStore.downloadPath = videoPath;
            notification.warning({
                content: "下载目录已不存在，已设置为默认下载目录",
                duration: 5000,
            });
        }
    }

    downloadingStore.init();
});

const inverted = ref(false); // 用于控制颜色反转

function renderIcon(icon) {
    return () => h(NIcon, null, { default: () => h(icon) });
}

// 菜单项配置
const menuOptions = [
    {
        label: () =>
            h(
                RouterLink,
                { to: { name: "DownloadList" } },
                { default: () => "下载列表" },
            ),
        key: "DownloadList",
        icon: renderIcon(CloudDownload),
    },
    {
        label: () =>
            h(
                RouterLink,
                { to: { name: "DownloadCompleted" } },
                { default: () => "下载完成" },
            ),
        key: "DownloadCompleted",
        icon: renderIcon(CloudDone),
    },
    {
        label: () =>
            h(
                RouterLink,
                { to: { name: "Setting" } },
                { default: () => "软件设置" },
            ),
        key: "Setting",
        icon: renderIcon(Cog),
    },
];

listen("open_settings", () => {
    router.push({ name: "Setting" });
});

// ==========================================
// m3u8dl:// 深链下载处理
// ==========================================
// 后端将深链解析结果入队并通过事件通知，这里拉取队列并加入下载列表。
// 事件与“挂载时拉取”双通道，保证冷启动/运行中唤起都不会漏掉任务。
const processDeepLinkQueue = async () => {
    try {
        const items = await invoke("drain_pending_deep_links");
        if (!Array.isArray(items) || items.length === 0) return;

        for (const item of items) {
            if (settingStore.deepLinkAutoDownload) {
                // 设置开启：直接加入列表并开始下载
                downloadingStore.addDeepLinkDownload(item);
            } else {
                // 默认：交给下载列表弹出「新建下载」界面，由用户确认后再下载
                UIStore.pushDeepLink(item);
            }
        }

        router.push({ name: "DownloadList" });
    } catch (e) {
        console.error("处理深链下载失败:", e);
    }
};

let unlistenDeepLink = null;
onMounted(async () => {
    unlistenDeepLink = await listen("deep_link_download", () => {
        processDeepLinkQueue();
    });
    // 冷启动场景：应用启动时即携带深链
    processDeepLinkQueue();
});

onUnmounted(() => {
    unlistenDeepLink?.();
});

// 路由映射表：路由名称 -> URL hash
const routeHashMap = {
    DownloadList: "dList",
    DownloadCompleted: "dCompleted",
    Setting: "setting",
};

// 监听路由变化，保存当前路由到 store，以便重建窗口时恢复
router.afterEach((to) => {
    const hash = routeHashMap[to.name];
    if (hash) {
        settingStore.lastRoute = hash;
    }
});
</script>

<template>
    <n-space vertical>
        <n-layout>
            <!--      <n-layout-header :inverted="inverted">-->
            <!--      </n-layout-header>-->
            <n-layout has-sider>
                <n-layout-sider
                    bordered
                    show-trigger
                    collapse-mode="width"
                    :collapsed="UIStore.collapsed"
                    :collapsed-width="64"
                    :width="200"
                    :native-scrollbar="false"
                    :inverted="inverted"
                    @update:collapsed="UIStore.toggleCollapsed"
                    style="
                        position: fixed;
                        top: 0;
                        left: 0;
                        height: 100vh;
                        z-index: 100;
                    "
                >
                    <n-menu
                        :inverted="inverted"
                        :collapsed-width="64"
                        :collapsed-icon-size="22"
                        :options="menuOptions"
                        :value="String(router.currentRoute.value.name)"
                    />
                </n-layout-sider>
                <n-layout
                    :style="{
                        marginLeft: UIStore.collapsed ? '64px' : '200px',
                        minHeight: '100vh',
                        padding: '0.8rem',
                        backgroundColor: '#faf9f8',
                    }"
                >
                    <n-modal-provider>
                        <RouterView />
                    </n-modal-provider>
                </n-layout>
            </n-layout>
            <!--      <n-layout-footer :inverted="inverted">-->
            <!--      </n-layout-footer>-->
        </n-layout>
    </n-space>
</template>

<style scoped></style>
