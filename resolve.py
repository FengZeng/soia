import os
files = ["src-tauri/src/store/ui_state_store.rs", "src/App.vue", "src/components/player-controls/RightControls.vue", "src/panels/SettingsPanel.vue", "src/styles/panels.css"]
for f in files:
    with open(f, 'r') as fh:
        content = fh.read()
    if '<<<<<<<' in content:
        print(f"\n--- Conflicts in {f} ---")
        lines = content.splitlines()
        in_conflict = False
        for i, line in enumerate(lines):
            if line.startswith('<<<<<<<'):
                in_conflict = True
                print(f"Line {i+1}: {line}")
            elif line.startswith('======='):
                print(f"Line {i+1}: {line}")
            elif line.startswith('>>>>>>>'):
                print(f"Line {i+1}: {line}")
                in_conflict = False
            elif in_conflict:
                print(line)
