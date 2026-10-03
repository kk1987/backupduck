#!/usr/bin/env python3
"""The mobile native libraries must not link the optional desktop engines."""
import subprocess
for target in ['aarch64-apple-ios-sim','aarch64-linux-android']:
    tree=subprocess.check_output(['cargo','tree','--locked','-p','backupduck-native','--no-default-features','--target',target,'--prefix','none'],text=True)
    assert 'backupduck-folder-source' not in tree, f'Folder module leaked into {target}'
    assert 'backupduck-cloud-audit' not in tree, f'Cloud audit leaked into {target}'
print('Folder and cloud-audit capability isolation passed for iOS and Android')
