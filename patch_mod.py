import sys

with open("src/web/mod.rs", "r") as f:
    content = f.read()

content = content.replace("pub mod partials;", "pub mod partials;\npub mod recovery;")
content = content.replace(".route(\"/analytics\", get(pages::analytics))", ".route(\"/analytics\", get(pages::analytics))\n        .route(\"/recovery\", get(recovery::recovery_page))")

with open("src/web/mod.rs", "w") as f:
    f.write(content)
print("Mod replaced successfully")
