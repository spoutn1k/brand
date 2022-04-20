import os
import sys
import pathlib
import json
import tempfile
import logging
from shlex import split
from subprocess import Popen, PIPE

SUCCESS = 0
FAILURE = 1

TEMPLATE = """
{
  "Make": "NIKON CORPORATION",
  "Model": "NIKON N2000",
  "ShutterSpeed": "1/500",
  "ExposureTime": "1/500",
  "FNumber": 5.6,
  "Aperture": 5.6,
  "ISOSetting": 360,
  "ISO": 360,
  "FocalLength": "105.0 mm",
  "Exif:Description": "",
}
"""


class Record:
    empty = 'N/A'
    fields = ['fnumber', 'sspeed', 'flength', 'comment', 'date', 'coordinates']

    def __init__(self, line: str):
        for field in Record.fields:
            setattr(self, field, Record.empty)

        for (k, v) in zip(Record.fields, line.split('\t')):
            setattr(self, k, v)

    def export(self):
        coordinates = {}

        if self.coordinates and self.coordinates != Record.empty:
            try:
                lat, lon = self.coordinates.split(',')
            except:
                print("Badly formatted coordinates: %s" % self.coordinates)
                sys.exit(1)

            coordinates = {
                "GPSLatitude": lat,
                "GPSLatitudeRef": 'N',
                "GPSLongitude": lon,
                "GPSLongitudeRef": 'W',
            }

        return coordinates | {
            "shutterspeed": self.sspeed,
            "ApertureValue": self.fnumber,
            "FNumber": self.fnumber,
            "focallength": self.flength,
            "Description": self.comment,
            "alldates": self.date,
        }


def get_index(filename):
    return int(pathlib.Path(filename).stem[0:4].strip('_').strip('A'))


def read_tse(filename):
    with open(filename, 'r') as tse:
        lines = [l.strip('\n') for l in tse.readlines()]

    metadata = list(filter(lambda l: l[0] == '#', lines))
    lines = list(filter(lambda l: l[0] not in ['#', ';'], lines))

    template = {}
    for field in metadata:
        els = field.split()
        template[els[0].strip('#')] = ' '.join(els[1:])

    return template, lines


def main():
    negatives_dir = pathlib.Path(sys.argv[1])
    if not (negatives_dir.exists() and negatives_dir.is_dir()):
        return FAILURE

    _, _, filenames = next(os.walk(negatives_dir), (None, None, []))

    MAX = 40
    present = {}

    for i in range(MAX):
        present[i] = set()

    for filename in filter(
            lambda p: pathlib.Path(p).suffix.lower() in ['.jpg', '.tiff'],
            filenames):
        try:
            index = get_index(filename)
        except ValueError:
            continue
        present[index].add(filename)

    template, exposures = read_tse(sys.argv[2])

    todo = []

    for i in range(1, 41):
        if not present.get(i) or len(exposures) < i:
            continue

        data = template.copy()

        data.update(Record(exposures[i - 1]).export())

        datafile = tempfile.NamedTemporaryFile('w+', delete=False)
        datafile.write(json.dumps(data, indent=1))
        datafile.close()

        for filename in present.get(i):
            todo.append((filename, datafile.name))

    logging.info("Tagging %d files" % len(todo))
    processes = []
    for filename, data in todo:
        command = f"exiftool -m -q -j={data} {os.getcwd()}/{negatives_dir}/{filename}"
        processes.append(Popen(split(command), stdout=PIPE, stderr=PIPE))

    for proc in procrocesses:
        if status := proc.wait():
            logging.critical("Process %d failed with status %d: %s", proc.pid,
                             proc.returncode, proc.stderr)

    for _, datafile in todo:
        if pathlib.Path(datafile).exists():
            os.unlink(datafile)


if __name__ == '__main__':
    logging.basicConfig(level=logging.INFO)

    if len(sys.argv) < 3:
        print("Usage: %s <negatives> <tse file>" % sys.argv[0])
        sys.exit(FAILURE)

    main()
