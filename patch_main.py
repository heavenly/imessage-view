import sys

with open("src/main.rs", "r") as f:
    content = f.read()

content = content.replace("db::queries::get_missing_attachments(&conn)?", "db::queries::get_missing_attachments(&conn, 0, i64::MAX)?")

with open("src/main.rs", "w") as f:
    f.write(content)
print("Main replaced successfully")
