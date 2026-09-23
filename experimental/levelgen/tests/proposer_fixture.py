"""Local proposal fixture; no model calls or credentials."""
import json
import sys

context = json.load(sys.stdin)
recipe = context["incumbent_recipe"].copy()
recipe["rock_percent"] += 3
print(json.dumps({"hypothesis": "Test a denser initial cave field.", "recipe": recipe}))
