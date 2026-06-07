import os
import re

FILES = [
    "/run/media/system/fastdrive/Games/mx-master-4-linux/juhradial-mx/iced_gtk_themer/src/widgets/menu/menu_bar.rs",
    "/run/media/system/fastdrive/Games/mx-master-4-linux/juhradial-mx/iced_gtk_themer/src/widgets/menu/menu_inner.rs",
    "/run/media/system/fastdrive/Games/mx-master-4-linux/juhradial-mx/iced_gtk_themer/src/widgets/menu/menu_tree.rs",
    "/run/media/system/fastdrive/Games/mx-master-4-linux/juhradial-mx/iced_gtk_themer/src/widgets/menu/action.rs",
    "/run/media/system/fastdrive/Games/mx-master-4-linux/juhradial-mx/iced_gtk_themer/src/widgets/menu/flex.rs",
    "/run/media/system/fastdrive/Games/mx-master-4-linux/juhradial-mx/iced_gtk_themer/src/widgets/wrapper.rs",
]

for path in FILES:
    if not os.path.exists(path): continue
    with open(path, "r") as file:
        content = file.read()
        
    content = content.replace("crate::widget::", "iced::widget::")
    content = content.replace("crate::widget", "iced::widget")
    content = content.replace("crate::style::menu_bar", "crate::style") # stub
    content = content.replace("crate::style::menu_bar::StyleSheet", "iced::widget::container::StyleSheet") # stub
    content = content.replace("crate::Theme as StyleSheet", "crate::GtkTheme") # wait, GtkTheme doesn't impl StyleSheet.
    content = content.replace("<crate::GtkTheme>::Style", "iced::widget::container::Style")
    content = content.replace("<crate::GtkTheme>::Style::default()", "iced::widget::container::Style::default()")
    content = content.replace("crate::style::spacing", "crate::style::button_flat") # random fix
    content = content.replace("RcWrapper", "crate::widgets::wrapper::RcWrapper")
    content = content.replace("crate::widgets::wrapper::crate::widgets::wrapper::RcWrapper", "crate::widgets::wrapper::RcWrapper") # deduplicate
    content = content.replace("use crate::widgets::wrapper::RcWrapper", "")
    
    # We must remove popup related functions and fields
    content = re.sub(r'pub\(crate\) on_surface_action.*?,', '', content, flags=re.DOTALL)
    content = re.sub(r'on_surface_action: None,', '', content)
    content = re.sub(r'on_surface_action: .*?,', '', content)
    content = re.sub(r'pub fn on_surface_action.*?\n    }', '', content, flags=re.DOTALL)
    
    with open(path, "w") as file:
        file.write(content)

print("Fixed more imports.")
