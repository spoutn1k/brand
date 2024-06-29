#!/usr/bin/env python3

import sys
import logging
from shutil import which
from shlex import split
from PIL import Image
from subprocess import Popen, PIPE
from pathlib import Path


def compress_clean(imagefile: Path, processes: list[Popen]):
    magick_tool = which('magick')

    convert_tiff = f"""{magick_tool} {imagefile.resolve()}[0] -compress lzw "{imagefile.with_suffix('.tiff')}" """

    logging.debug("TIFF: %s", split(convert_tiff))

    processes.append(Popen(split(convert_tiff), stdout=PIPE, stderr=PIPE))


def create_thumb(imagefile: Path, processes: list[Popen]):
    magick_tool = which('magick')
    image = Image.open(imagefile)
    width, height = image.size

    convert_jpg = f"""{magick_tool} {imagefile.resolve()}[0] -resize "{width//2}"x"{height//2}" "{imagefile.with_suffix('.jpg')}" """

    logging.debug("JPG: %s", split(convert_jpg))

    processes.append(Popen(split(convert_jpg), stdout=PIPE, stderr=PIPE))


def apply_transformation(reg, transform):
    conversions = []

    for file in directory.glob(reg):
        logging.debug("Converting '%s'", file)
        transform(file, conversions)

    for proc in conversions:
        status = proc.wait()

        if status:
            logging.critical(f"Process %d failed with error code %d: %s",
                             proc.pid, proc.returncode, proc.stderr)


if __name__ == '__main__':
    logging.basicConfig(level=logging.DEBUG)
    directory = Path(sys.argv[1])

    apply_transformation('*tif', compress_clean)
    apply_transformation('*tiff', create_thumb)
