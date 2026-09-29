import re

def resolve_ui_state(content):
    # Just need to keep surround_sound block and use upstream's playback_adjustments block
    pattern = re.compile(r'<<<<<<< HEAD\n(.*?)\n=======\n(.*?)>>>>>>> upstream/main\n', re.DOTALL)
    def repl(m):
        head = m.group(1)
        upstream = m.group(2)
        if "surround_sound:" in head and "playback_adjustments:" in upstream:
            # head contains surround_sound + old playback_adjustments
            # upstream contains new playback_adjustments
            surround_match = re.search(r'(surround_sound:.*?),\n\s*playback_adjustments:', head, re.DOTALL)
            if surround_match:
                surround = surround_match.group(1)
                return f"{surround},\n{upstream}"
        
        if "virtual_surround_enabled" in head and "title_bar_style" in upstream:
            # We must merge struct fields or Default impl
            # Let's just combine them (assuming they are lines)
            return head + "\n" + upstream
        return head + "\n" + upstream
    
    return pattern.sub(repl, content)

def resolve_app(content):
    pattern = re.compile(r'<<<<<<< HEAD\n(.*?)\n=======\n(.*?)>>>>>>> upstream/main\n', re.DOTALL)
    def repl(m):
        return m.group(1) + "\n" + m.group(2)
    return pattern.sub(repl, content)

def resolve_right_controls(content):
    pattern = re.compile(r'<<<<<<< HEAD\n(.*?)\n=======\n(.*?)>>>>>>> upstream/main\n', re.DOTALL)
    def repl(m):
        return m.group(1) + "\n" + m.group(2)
    return pattern.sub(repl, content)

def resolve_settings_panel(content):
    pattern = re.compile(r'<<<<<<< HEAD\n(.*?)\n=======\n(.*?)>>>>>>> upstream/main\n', re.DOTALL)
    def repl(m):
        return m.group(1) + "\n" + m.group(2)
    return pattern.sub(repl, content)

with open('src-tauri/src/store/ui_state_store.rs', 'r') as f:
    content = f.read()
with open('src-tauri/src/store/ui_state_store.rs', 'w') as f:
    f.write(resolve_ui_state(content))

with open('src/App.vue', 'r') as f:
    content = f.read()
with open('src/App.vue', 'w') as f:
    f.write(resolve_app(content))

with open('src/components/player-controls/RightControls.vue', 'r') as f:
    content = f.read()
with open('src/components/player-controls/RightControls.vue', 'w') as f:
    f.write(resolve_right_controls(content))

with open('src/panels/SettingsPanel.vue', 'r') as f:
    content = f.read()
with open('src/panels/SettingsPanel.vue', 'w') as f:
    f.write(resolve_settings_panel(content))
