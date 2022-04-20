#!/usr/bin/env python3

import logging
from shutil import which
from shlex import split
from PIL import Image
from subprocess import Popen, PIPE
from pathlib import Path


def gen_converts(imagefile: Path, processes: list[Popen]):
    magick_tool = which('convert')
    image = Image.open(imagefile)
    width, height = image.size

    convert_tiff = f"""{magick_tool} {imagefile.resolve()}[0] -compress lzw "{imagefile.with_suffix('.tiff')}" """
    convert_jpg = f"""{magick_tool} {imagefile.resolve()}[0] -resize "{width//2}"x"{height//2}" "{imagefile.with_suffix('.jpg')}" """

    logging.debug("TIFF: %s", split(convert_tiff))
    logging.debug("JPG: %s", split(convert_jpg))

    processes.append(Popen(split(convert_tiff), stdout=PIPE, stderr=PIPE))
    processes.append(Popen(split(convert_jpg), stdout=PIPE, stderr=PIPE))


if __name__ == '__main__':
    logging.basicConfig(level=logging.INFO)

    conversions = []

    import sys
    for tif in Path(sys.argv[1]).glob('*tif'):
        logging.debug("Converting '%s'", tif)
        gen_converts(tif, conversions)

    for proc in conversions:
        status = proc.wait()

        if status:
            logging.critical(f"Process {proc.pid} failed: {p.stderr}")
