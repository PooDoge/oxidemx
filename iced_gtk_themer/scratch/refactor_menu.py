import os

FILES = [
    "/run/media/system/fastdrive/Games/mx-master-4-linux/juhradial-mx/iced_gtk_themer/src/widgets/menu.rs",
    "/run/media/system/fastdrive/Games/mx-master-4-linux/juhradial-mx/iced_gtk_themer/src/widgets/wrapper.rs",
]

for path in FILES:
    if not os.path.exists(path): continue
    with open(path, "r") as file:
        content = file.read()
        
    content = content.replace("crate::Theme", "crate::GtkTheme")
    content = content.replace("crate::Renderer", "iced::Renderer")
    content = content.replace("crate::theme", "crate::style")
    content = content.replace("crate::surface", "crate::surface")
    content = content.replace("crate::Element", "iced::Element")
    content = content.replace("crate::action", "crate::action")
    content = content.replace("crate::widget::RcWrapper", "crate::widgets::wrapper::RcWrapper")
    
    with open(path, "w") as file:
        file.write(content)

print("Replaced basic types in menu.rs and wrapper.rs.")
