#!/usr/bin/python3

import redis, sys, os, webp
from tgs import parsers
from tgs.exporters.cairo import export_png

redis_addr = sys.argv[1]

tgs_key = sys.argv[2]
output_key = sys.argv[3]

conn = redis.Redis().from_url("redis://@{}/0".format(redis_addr))
data = conn.get(tgs_key)

# for some reason you can't parse a tgs from already loaded data so save it to a file..
tgs_file = "/tmp/{}.tgs".format(output_key)

with open(tgs_file, "wb") as f:
    f.write(data)

image = parsers.tgs.parse_tgs(tgs_file)

# taken from the tgs2svg script, no idea why this works but it does
half_way = image.out_point / 50

# again it doesn't let me write to memory so output to a file...
png_file = "/tmp/{}.png".format(output_key)
export_png(image, png_file, half_way)

# import to Redis
png_data = ()
with open(png_file, "rb") as f:
    png_data = f.read()
    conn.set(output_key, png_data)

# cleanup
os.remove(tgs_file)
os.remove(png_file)
