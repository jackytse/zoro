/**
 * English message pack.
 */
export default {
  app: {
    toLightMode: "Switch to light mode",
    toDarkMode: "Switch to dark mode",
  },

  time: {
    never: "Never opened",
    justNow: "Just now",
    minutesAgo: "{n} min ago",
    hoursAgo: "{n} h ago",
  },

  tray: {
    recentProjects: "Recent Projects",
    noProjectTitle: "No projects configured yet",
    noProjectHint:
      "Add your frequently used project paths in Settings, then they will show up here.",
    goSettings: "Open Settings",
    unknownTool: "Unknown tool",
    openInTerminal: "Open in terminal",
    openInTerminalNamed: "Open {name} in terminal",
    loadConfigFailed: "Failed to load configuration",
    openFailed: "Cannot open \"{name}\" with {tool}",
    openSettingsFailed: "Failed to open Settings",
    quitFailed: "Failed to quit",
    terminalOpenFailed: "Failed to open in terminal",
    settings: "Settings",
    quit: "Quit",
    quitApp: "Quit application",
  },

  settings: {
    title: "Settings",
    subtitle:
      "Configure your development tools and project paths, then open them quickly from the tray popup.",

    addProject: "Add Project",
    projectName: "Project name",
    nameHint: "Leave empty to use the folder name",
    namePlaceholder: "My project",
    tool: "Development tool",
    projectPath: "Project path",
    pathHint: "Supports ~ prefix, e.g. ~/Projects/zoro",
    browse: "Browse folder",
    addToTray: "Add to tray",
    pathRequired: "Project path is required",
    toolRequired: "Please select a development tool",
    pickFolder: "Choose project folder",
    browseFailed: "Failed to open folder picker",
    added: "Project added",
    addFailed: "Failed to add project",

    import: "Import from dev tools",
    scan: "Scan recent projects",
    importDesc:
      "Reads recent workspaces from VS Code / Cursor and recent projects from JetBrains IDEs (read-only, never writes to these tools). Deleted folders and already configured paths are filtered out; iTerm keeps no readable history and is not scanned.",
    scanEmpty: "No new recent projects found",
    scanEmptyDesc:
      "All records from local tools are already configured, or these tools haven't been used on this machine.",
    scanFailed: "Scan failed",
    scanCount: "{n} items",
    importTitle: "Import Recent Projects",
    importPickDesc: "Select projects to import",
    selectAll: "Select all",
    importN: "Import {n} items",
    imported: "Imported {n} projects",
    importFailures: "{n} failed, details follow",

    configuredProjects: "Configured Projects ({n})",
    emptyProjects: "No projects yet. Add your first one with the form above.",
    tryOpen: "Try open",
    removeProject: "Remove project",
    removed: "Removed \"{name}\"",
    removeFailed: "Failed to remove",
    openTryFailed: "Failed to open \"{name}\"",
    openedAt: "opened {time}",

    tools: "Available tools",
    toolsDesc:
      "Open order: run the CLI first, resolve via login shell when missing, finally fall back to open -a with the app name.",

    general: "General",
    autostart: "Launch at login",
    autostartDesc: "Start automatically after login and minimize to tray without showing the window",
    autostartOn: "Launch at login enabled",
    autostartOff: "Launch at login disabled",
    terminalMode: "Terminal open mode",
    terminalModeDesc: "Whether the terminal icon in recent projects opens a new window or a new tab",
    terminalWindow: "New window",
    terminalTab: "New tab",
    terminalModeFailed: "Failed to set terminal open mode",
    language: "Language",
    languageDesc: "UI display language, defaults to your system language",
    followSystem: "System",
    setFailed: "Failed to save settings",

    shortcut: "Global Shortcut",
    shortcutHint: "Open the recent projects popup",
    shortcutDisabled: "Disabled",
    pressKeys: "Press a key combination… (Esc to cancel)",
    shortcutDesc:
      "Platform default {default}; combinations must include a modifier key. Letters, digits and F1-F12 are supported. Registration failures usually mean the combination is taken by another app.",
    cancel: "Cancel",
    save: "Save",
    reset: "Reset to default",
    disable: "Disable",
    edit: "Change",
    shortcutUpdated: "Global shortcut updated",
    shortcutSetFailed: "Failed to set shortcut",
    shortcutReset: "Shortcut reset to default",
    shortcutDisabledToast: "Global shortcut disabled",
    actionFailed: "Operation failed",
  },
} as const;
