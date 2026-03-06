import sys

with open("templates/base.html", "r") as f:
    content = f.read()

content = content.replace("<li><a href=\"/analytics\">Analytics</a></li>", "<li><a href=\"/analytics\">Analytics</a></li>\n            <li><a href=\"/recovery\">Recovery</a></li>")

with open("templates/base.html", "w") as f:
    f.write(content)
print("Base replaced successfully")
