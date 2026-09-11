import matplotlib.pyplot as plt
import numpy as np

# def gain(det):
#     x = 20 * np.log10(max(det, 1e-12))
#
#     t = -6       # threshold
#     k = 6        # knee width
#     r = 4        # expansion ratio
#
#     d = x - t
#
#     if d >= k / 2:
#         # Above threshold: no expansion
#         y = x
#
#     elif d <= -k / 2:
#         # Below threshold: downward expansion
#         y = t + d * r
#
#     else:
#         # Soft knee
#         y = x + (r - 1) * (d - k / 2)**2 / (2 * k)
#
#     return 10**((y - x) / 20)
#
#
# def f(x):
#     return gain(x) * x


def gain(det):
    x = np.log10(det) * 20

    t = -6
    k = 6
    r = 3.4
    expand = False

    d = x - t
    y = x
    if expand:
        if d < k / 2:
            y = t + d * r
        if abs(d) <= k / 2:
            y = x + (1 - r) * (d - k / 2) * (d - k / 2) / (2 * k)
        if d > k / 2:
            y = x
    else:
        y = x
        if abs(d) <= k / 2:
            print(d)
            y = x + (1 / r - 1) * (d + k / 2) * (d + k / 2) / (2 * k)
        elif d > k / 2:
            y = t + d / r

    yg = y - x
    return np.pow(10, yg / 20)


def f(x):
    x = x * 5
    return gain(x) * x


data = [f(x / 1000) for x in range(1, 1000)]
data_base = [x / 1000 for x in range(1, 1000)]
time = np.linspace(0, 1, num=len(data))

plt.figure(figsize=(10, 6))
plt.plot(time, data, alpha=1.0, color="blue")
plt.plot(time, data_base, alpha=1.0, color="orange")
plt.grid(True)

plt.tight_layout()
plt.show()
