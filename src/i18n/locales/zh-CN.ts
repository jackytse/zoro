/**
 * 简体中文语言包（基准语言，key 齐全；其他语言缺 key 时回落到这里）。
 */
export default {
  app: {
    toLightMode: "切换到浅色模式",
    toDarkMode: "切换到深色模式",
  },

  time: {
    never: "未打开过",
    justNow: "刚刚",
    minutesAgo: "{n} 分钟前",
    hoursAgo: "{n} 小时前",
  },

  tray: {
    recentProjects: "最近项目",
    noProjectTitle: "还没有配置项目",
    noProjectHint:
      "在主窗口的「设置」里添加常用项目路径并选择开发工具，就会出现在这里。",
    goSettings: "去设置",
    unknownTool: "未知工具",
    openInTerminal: "在终端打开",
    openInTerminalNamed: "在终端打开 {name}",
    // toast
    loadConfigFailed: "读取配置失败",
    openFailed: "无法用 {tool} 打开「{name}」",
    openSettingsFailed: "打开设置失败",
    quitFailed: "退出失败",
    terminalOpenFailed: "在终端打开失败",
    settings: "设置",
    quit: "退出",
    quitApp: "退出应用",
    checkUpdate: "检测更新",
    quitConfirmHint: "再按一次 ⌘Q 确认退出",
  },

  settings: {
    title: "设置",
    subtitle: "配置常用开发工具与项目路径，配置完成后可在托盘弹窗中快速打开。",

    // 添加项目
    addProject: "添加项目",
    projectName: "项目名称",
    nameHint: "留空时自动取路径末段作为项目名",
    namePlaceholder: "我的项目",
    tool: "开发工具",
    projectPath: "项目路径",
    pathHint: "支持 ~ 前缀，如 ~/Projects/zoro",
    browse: "浏览目录",
    addToTray: "添加到托盘",
    pathRequired: "请填写项目路径",
    toolRequired: "请选择开发工具",
    pickFolder: "选择项目目录",
    browseFailed: "打开目录选择器失败",
    added: "已添加项目",
    addFailed: "添加失败",

    // 从开发工具导入
    import: "从开发工具导入",
    scan: "扫描最近项目",
    importDesc:
      "读取 VS Code / Cursor 的最近工作区与 JetBrains 系列产品的最近项目记录（只读，不会写入这些工具的数据）。已自动过滤已删除目录和配置过的路径；iTerm 没有可读取的记录，未参与扫描。",
    scanEmpty: "没有扫描到新的最近项目",
    scanEmptyDesc: "本机工具的记录都已配置过，或这些工具尚未在本机使用。",
    scanFailed: "扫描失败",
    scanCount: "{n} 条",
    importTitle: "导入最近项目",
    importPickDesc: "选择要导入的项目",
    selectAll: "全选",
    importN: "导入 {n} 项",
    imported: "已导入 {n} 个项目",
    importFailures: "{n} 条失败，详情见后续提示",

    // 已配置项目
    configuredProjects: "已配置项目（{n}）",
    emptyProjects: "还没有项目，用上面的表单添加第一个吧。",
    tryOpen: "试运行打开",
    removeProject: "移除项目",
    removed: "已移除「{name}」",
    removeFailed: "移除失败",
    openTryFailed: "打开「{name}」失败",
    openedAt: "{time}打开",

    // 可用工具
    tools: "可用工具",
    toolsDesc: "打开顺序：优先执行 CLI，找不到时通过登录 shell 解析，最后回退到 open -a 应用名。",

    // 通用
    general: "通用",
    // 设置页分组 Tabs
    tabGeneral: "通用",
    tabProjects: "项目",
    tabTools: "工具",
    // 外观
    appearance: "外观",
    theme: "主题",
    themeDesc: "界面明暗主题，默认跟随系统，系统切换时实时跟随",
    themeSystem: "跟随系统",
    themeLight: "浅色",
    themeDark: "深色",
    // 启动
    launch: "启动",
    // 终端分组标题
    terminalSection: "终端",
    autostart: "开机自动启动",
    autostartDesc: "登录系统后自动运行并最小化到托盘，不弹出主窗口",
    autostartOn: "已开启开机自启",
    autostartOff: "已关闭开机自启",
    projectTool: "打开工具",
    defaultTerminal: "默认终端",
    defaultTerminalDesc: "在终端打开项目时使用的终端应用；自动推荐会优先选择已安装的常用终端",
    terminalAuto: "自动推荐",
    notInstalled: "未安装",
    defaultTerminalFailed: "设置默认终端失败",
    changeToolFailed: "更换打开工具失败",
    terminalMode: "终端打开方式",
    terminalModeDesc: "最近项目里点终端图标时，在新窗口还是新选项卡打开",
    terminalWindow: "新窗口",
    terminalTab: "新选项卡",
    terminalModeFailed: "设置终端打开方式失败",
    language: "语言",
    languageDesc: "界面显示语言，默认跟随系统语言",
    followSystem: "跟随系统",
    setFailed: "设置失败",

    // 全局快捷键
    shortcut: "全局快捷键",
    shortcutHint: "呼出最近项目弹窗",
    shortcutDisabled: "已禁用",
    pressKeys: "按下组合键…（Esc 取消）",
    shortcutDesc:
      "平台默认 {default}；组合需包含修饰键，支持字母 / 数字 / F1-F12。注册失败通常意味着组合被其他应用占用。",
    cancel: "取消",
    save: "保存",
    reset: "恢复默认",
    disable: "禁用",
    edit: "修改",
    shortcutUpdated: "全局快捷键已更新",
    shortcutSetFailed: "设置快捷键失败",
    shortcutReset: "已恢复默认快捷键",
    shortcutDisabledToast: "全局快捷键已禁用",
    actionFailed: "操作失败",

    // 关于与更新
    aboutSection: "关于与更新",
    currentVersion: "当前版本",
    currentVersionDesc: "安装包与更新清单由 CNB Release 托管，更新包经 minisign 签名校验",
    updateChannel: "更新通道",
    updateChannelDesc: "预发布通道会提前体验新功能，但可能不够稳定",
    updateChannelStable: "正式版",
    updateChannelBeta: "预发布",
    checkUpdate: "检测更新",
    upToDate: "已是最新版本",
    newVersionFound: "发现新版本 v{version}",
    updateNow: "立即更新",
    downloading: "下载中 {progress}",
    installedRestart: "更新已安装，重启应用生效",
    restart: "重启应用",
    updateCheckFailed: "检测更新失败",
    updateInstallFailed: "下载或安装更新失败",
    updateSourceUnavailable: "更新源暂不可用：当前通道还没有包含更新清单的发布（更新清单托管在 CNB 仓库 updater 分支）",
    sourceRepo: "源码仓库",
    sourceRepoDesc: "CNB 为发布主渠道，GitHub 为源码镜像",
    openRepoFailed: "打开仓库链接失败",
  },
} as const;
