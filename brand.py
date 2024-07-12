import os
import json
import logging
from tempfile import NamedTemporaryFile
from pathlib import Path
from shlex import split
from subprocess import Popen, PIPE
from itertools import chain

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

    def __init__(self):
        for field in Record.fields:
            setattr(self, field, Record.empty)

    @classmethod
    def create_from(cls, line: str):
        record = cls()

        for (k, v) in zip(Record.fields, line.split('\t')):
            setattr(record, k, v)

        return record

    def export(self):
        coordinates = {}

        if self.coordinates and self.coordinates != Record.empty:
            try:
                lat, lon = self.coordinates.split(',')
                la_ref, lo_ref = 'N', 'E'
                if lat.startswith('-'):
                    la_ref = 'S'
                if lon.startswith('-'):
                    lo_ref = 'W'
                coordinates = {
                    "GPSLatitude": lat,
                    "GPSLatitudeRef": la_ref,
                    "GPSLongitude": lon,
                    "GPSLongitudeRef": lo_ref,
                }
            except:
                logging.error("Badly formatted coordinates: %s",
                              self.coordinates)

        return coordinates | {
            "shutterspeed": self.sspeed,
            "ApertureValue": self.fnumber,
            "FNumber": self.fnumber,
            "focallength": self.flength,
            "Description": self.comment,
            "alldates": self.date,
        }


def get_index(filename):
    index = int(Path(filename).stem[0:4].strip('_').strip('A'))
    logging.info("File '%s' has index '%d'", filename, index)
    return index


def read_tse(filename: Path):
    with open(filename, 'r', encoding='utf-8') as tse:
        lines = list(map(lambda l: l.strip('\n'), tse.readlines()))

    template = {}
    for field in filter(lambda l: l[0] == '#', lines):
        els = field.split()
        template[els[0].strip('#')] = ' '.join(els[1:])

    records = {}
    entries = filter(lambda l: l[0] not in ['#', ';'], lines)
    for index, value in enumerate(entries, start=1):
        records[index] = Record.create_from(value)

    return template, records


def main(negatives_dir: Path, tse_file: Path):
    filenames = list(negatives_dir.glob('*tiff')) + list(
        negatives_dir.glob('*jpg'))

    template, records = read_tse(tse_file)

    todo = []

    for filename in filenames:
        index = get_index(filename)

        if index not in records:
            logging.error("Missing exposure record for file '%s'", filename)
            continue

        data = template.copy()

        data.update(records[index].export())

        with NamedTemporaryFile('w+', delete=False) as datafile:
            json.dump(data, datafile)
            todo.append((filename, datafile.name))

    logging.info("Tagging %d files" % len(todo))
    processes = []

    for filename, data in todo:
        command = f"exiftool -m -q -j={data} {filename.resolve()}"
        processes.append(Popen(split(command), stdout=PIPE, stderr=PIPE))

    for proc in processes:
        if status := proc.wait():
            logging.critical("Process %d failed with status %d: %s", proc.pid,
                             proc.returncode, proc.stderr)

    for _, datafile in todo:
        if Path(datafile).exists():
            os.unlink(datafile)


if __name__ == '__main__':
    logging.basicConfig(level=logging.INFO)

    import sys

    if len(sys.argv) < 3:
        print("Usage: %s <negatives> <tse file>" % sys.argv[0])
        sys.exit(FAILURE)

    main(Path(sys.argv[1]), Path(sys.argv[2]))
