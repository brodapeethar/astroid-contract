with open("shared/src/errors.rs", "r") as f:
    text = f.read()

text = text.replace("NotAnApprover = 73,", "NotAnApprover = 73,\n    CancellationWindowClosed = 74,\n    MathOverflow = 75,\n    DivisionByZero = 76,")

with open("shared/src/errors.rs", "w") as f:
    f.write(text)
