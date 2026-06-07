with open("src/widgets/grid.rs", "r") as f:
    text = f.read()

text = text.replace(") -> Node {    let max_size", ") -> Node {\n    let max_size")
text = text.replace(") -> Node {(", ") -> Node {")

with open("src/widgets/grid.rs", "w") as f:
    f.write(text)
