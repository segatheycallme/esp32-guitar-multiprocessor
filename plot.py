import matplotlib.pyplot as plt
import numpy as np
from scipy.io import wavfile

x = 0
y = 2000

sample_rate1, data1 = wavfile.read("in.wav")
sample_rate2, data2 = wavfile.read("out.wav")

data1 = data1[x:y]
data2 = data2[x:y]

dur = len(data1) / sample_rate1 + x / sample_rate1
time1 = np.linspace(x / sample_rate1, dur, num=len(data1))
time2 = np.linspace(x / sample_rate1, dur, num=len(data2))


plt.figure(figsize=(10, 6))
plt.plot(time1, data1, alpha=1.0, color="blue")
plt.plot(time2, data2, alpha=1.0, color="red")
plt.grid(True)

plt.tight_layout()
plt.show()
