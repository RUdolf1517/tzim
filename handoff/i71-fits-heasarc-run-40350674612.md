<!-- tzim-ci:fits-heasarc:245a13f -->
### CI failure: `fits-heasarc`

# FITS gate diagnostics

Run: https://github.com/RUdolf1517/tzim/actions/runs/40350674612

## Literal 80-column header diffs

The comparison is original HDU versus the bytes produced by `tzim` then `funpack -C`. It includes every 80-byte card through END, card order, comments, spacing, CHECKSUM, DATASUM, EXTEND and ZEXTEND; nothing is normalized or excluded. These diffs diagnose the complete round trip and do not by themselves assign a difference to one program. The raw source, tzim candidate, full JSON/text diff, and per-tool output are retained in the checksummed artifact.

### int16_primary.fits

0-PRIMARY: changed card; original index 1, reconstructed index 1; SIMPLE
  original: 'SIMPLE  =                    T / original primary logical comment               '
  reconstructed: 'SIMPLE  =                    T                                                  '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: changed card; original index 2, reconstructed index 2; BITPIX
  original: 'BITPIX  =                   16 / original primary BITPIX comment                '
  reconstructed: 'BITPIX  =                   16 / array data type                                '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: changed card; original index 3, reconstructed index 3; NAXIS
  original: 'NAXIS   =                    2 / original primary NAXIS comment                 '
  reconstructed: 'NAXIS   =                    2 / number of array dimensions                     '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: changed card; original index 4, reconstructed index 4; NAXIS1
  original: 'NAXIS1  =                  120 / original primary width comment                 '
  reconstructed: 'NAXIS1  =                  120                                                  '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: changed card; original index 5, reconstructed index 5; NAXIS2
  original: 'NAXIS2  =                  256 / original primary height comment                '
  reconstructed: 'NAXIS2  =                  256                                                  '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: dropped card; original index 6, reconstructed index None; EXTEND
  original: 'EXTEND  =                    T / original EXTEND comment                        '
  reconstructed: None
  classification: structural card differs (still a literal round-trip failure)

### float32_specials.fits

0-PRIMARY: changed card; original index 2, reconstructed index 2; BITPIX
  original: 'BITPIX  =                  -32                                                  '
  reconstructed: 'BITPIX  =                  -32 / array data type                                '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: changed card; original index 3, reconstructed index 3; NAXIS
  original: 'NAXIS   =                    2                                                  '
  reconstructed: 'NAXIS   =                    2 / number of array dimensions                     '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: dropped card; original index 6, reconstructed index None; EXTEND
  original: 'EXTEND  =                    T                                                  '
  reconstructed: None
  classification: structural card differs (still a literal round-trip failure)

### float64_extension.fits

1-IMAGE   ' / origina: changed card; original index 1, reconstructed index 1; XTENSION
  original: "XTENSION= 'IMAGE   ' / original XTENSION comment                                "
  reconstructed: "XTENSION= 'IMAGE'              / Image extension                                "
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE   ' / origina: changed card; original index 2, reconstructed index 2; BITPIX
  original: 'BITPIX  =                  -64 / original extension BITPIX comment              '
  reconstructed: 'BITPIX  =                  -64 / array data type                                '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE   ' / origina: changed card; original index 3, reconstructed index 3; NAXIS
  original: 'NAXIS   =                    2 / original extension NAXIS comment               '
  reconstructed: 'NAXIS   =                    2 / number of array dimensions                     '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE   ' / origina: changed card; original index 4, reconstructed index 4; NAXIS1
  original: 'NAXIS1  =                   48 / original extension width comment               '
  reconstructed: 'NAXIS1  =                   48                                                  '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE   ' / origina: changed card; original index 5, reconstructed index 5; NAXIS2
  original: 'NAXIS2  =                   96 / original extension height comment              '
  reconstructed: 'NAXIS2  =                   96                                                  '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE   ' / origina: changed card; original index 6, reconstructed index 6; PCOUNT
  original: 'PCOUNT  =                    0 / original PCOUNT comment                        '
  reconstructed: 'PCOUNT  =                    0 / number of parameters                           '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE   ' / origina: changed card; original index 7, reconstructed index 7; GCOUNT
  original: 'GCOUNT  =                    1 / original GCOUNT comment                        '
  reconstructed: 'GCOUNT  =                    1 / number of groups                               '
  classification: structural card differs (still a literal round-trip failure)

### multi_hdu.fits

0-PRIMARY: changed card; original index 2, reconstructed index 2; BITPIX
  original: 'BITPIX  =                   16                                                  '
  reconstructed: 'BITPIX  =                   16 / array data type                                '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: changed card; original index 3, reconstructed index 3; NAXIS
  original: 'NAXIS   =                    2                                                  '
  reconstructed: 'NAXIS   =                    2 / number of array dimensions                     '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: dropped card; original index 6, reconstructed index None; EXTEND
  original: 'EXTEND  =                    T                                                  '
  reconstructed: None
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE: changed card; original index 1, reconstructed index 1; XTENSION
  original: "XTENSION= 'IMAGE   '                                                            "
  reconstructed: "XTENSION= 'IMAGE'              / Image extension                                "
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE: changed card; original index 2, reconstructed index 2; BITPIX
  original: 'BITPIX  =                  -32                                                  '
  reconstructed: 'BITPIX  =                  -32 / array data type                                '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE: changed card; original index 3, reconstructed index 3; NAXIS
  original: 'NAXIS   =                    2                                                  '
  reconstructed: 'NAXIS   =                    2 / number of array dimensions                     '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE: changed card; original index 6, reconstructed index 6; PCOUNT
  original: 'PCOUNT  =                    0                                                  '
  reconstructed: 'PCOUNT  =                    0 / number of parameters                           '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE: changed card; original index 7, reconstructed index 7; GCOUNT
  original: 'GCOUNT  =                    1                                                  '
  reconstructed: 'GCOUNT  =                    1 / number of groups                               '
  classification: structural card differs (still a literal round-trip failure)

### double_ext.fits

0-PRIMARY: changed card; original index 2, reconstructed index 2; BITPIX
  original: 'BITPIX  =                   16                                                  '
  reconstructed: 'BITPIX  =                   16 / array data type                                '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: changed card; original index 3, reconstructed index 3; NAXIS
  original: 'NAXIS   =                    2                                                  '
  reconstructed: 'NAXIS   =                    2 / number of array dimensions                     '
  classification: structural card differs (still a literal round-trip failure)
0-PRIMARY: dropped card; original index 6, reconstructed index None; EXTEND
  original: 'EXTEND  =                    T                                                  '
  reconstructed: None
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE: changed card; original index 1, reconstructed index 1; XTENSION
  original: "XTENSION= 'IMAGE   '                                                            "
  reconstructed: "XTENSION= 'IMAGE'              / Image extension                                "
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE: changed card; original index 2, reconstructed index 2; BITPIX
  original: 'BITPIX  =                  -32                                                  '
  reconstructed: 'BITPIX  =                  -32 / array data type                                '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE: changed card; original index 3, reconstructed index 3; NAXIS
  original: 'NAXIS   =                    2                                                  '
  reconstructed: 'NAXIS   =                    2 / number of array dimensions                     '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE: changed card; original index 6, reconstructed index 6; PCOUNT
  original: 'PCOUNT  =                    0                                                  '
  reconstructed: 'PCOUNT  =                    0 / number of parameters                           '
  classification: structural card differs (still a literal round-trip failure)
1-IMAGE: changed card; original index 7, reconstructed index 7; GCOUNT
  original: 'GCOUNT  =                    1                                                  '
  reconstructed: 'GCOUNT  =                    1 / number of groups                               '
  classification: structural card differs (still a literal round-trip failure)
2-IMAGE: changed card; original index 1, reconstructed index 1; XTENSION
  original: "XTENSION= 'IMAGE   '                                                            "
  reconstructed: "XTENSION= 'IMAGE'              / Image extension                                "
  classification: structural card differs (still a literal round-trip failure)
2-IMAGE: changed card; original index 2, reconstructed index 2; BITPIX
  original: 'BITPIX  =                   16                                                  '
  reconstructed: 'BITPIX  =                   16 / array data type                                '
  classification: structural card differs (still a literal round-trip failure)
2-IMAGE: changed card; original index 3, reconstructed index 3; NAXIS
  original: 'NAXIS   =                    2                                                  '
  reconstructed: 'NAXIS   =                    2 / number of array dimensions                     '
  classification: structural card differs (still a literal round-trip failure)
2-IMAGE: changed card; original index 6, reconstructed index 6; PCOUNT
  original: 'PCOUNT  =                    0                                                  '
  reconstructed: 'PCOUNT  =                    0 / number of parameters                           '
  classification: structural card differs (still a literal round-trip failure)
2-IMAGE: changed card; original index 7, reconstructed index 7; GCOUNT
  original: 'GCOUNT  =                    1                                                  '
  reconstructed: 'GCOUNT  =                    1 / number of groups                               '
  classification: structural card differs (still a literal round-trip failure)

## registered multi-HDU positive traversal

Original fitsverify exit 0; candidate exit 0; funpack -C exit 0.
This fixture uses only registered IMAGE and BINTABLE extensions; the original must pass fitsverify before the candidate is interpreted.

Original HDU layout (byte offsets):
- HDU 0 PRIMARY: start=0, header=2880, data=16384, padded-data=17280, next=20160
- HDU 1 IMAGE: start=20160, header=2880, data=8192, padded-data=8640, next=31680
- HDU 2 BINTABLE: start=31680, header=2880, data=32, padded-data=2880, next=37440

Candidate HDU layout (byte offsets):
- HDU 0 PRIMARY: start=0, header=2880, data=0, padded-data=0, next=2880
- HDU 1 BINTABLE: start=2880, header=2880, data=7891, padded-data=8640, next=14400
- HDU 2 BINTABLE: start=14400, header=2880, data=6952, padded-data=8640, next=25920
- HDU 3 BINTABLE: start=25920, header=2880, data=32, padded-data=2880, next=31680

### fitsverify -l on original input

````text
 
              fitsverify 4.22 (CFITSIO V4.7.0)              
              --------------------------------              
 
 
File: /home/runner/work/_temp/fits-heasarc-work/scratch/original-multi_hdu.fits

3 Header-Data Units in this file.
 
=================== HDU 1: Primary Array ===================
 
   1 | SIMPLE  =                    T
   2 | BITPIX  =                   16
   3 | NAXIS   =                    2
   4 | NAXIS1  =                   64
   5 | NAXIS2  =                  128
   6 | EXTEND  =                    T
   7 | END     
 
 7 header keywords
 
 16-bit integer pixels,  2 axes (64 x 128), 
 
=================== HDU 2: Image Exten. ====================
 
   1 | XTENSION= 'IMAGE   '
   2 | BITPIX  =                  -32
   3 | NAXIS   =                    2
   4 | NAXIS1  =                   32
   5 | NAXIS2  =                   64
   6 | PCOUNT  =                    0
   7 | GCOUNT  =                    1
   8 | EXTNAME = 'SCI     '
   9 | END     
 
 9 header keywords
 
SCI 32-bit floating point pixels,  2 axes (32 x 64), 
 
=================== HDU 3: BINARY Table ====================
 
   1 | XTENSION= 'BINTABLE'
   2 | BITPIX  =                    8
   3 | NAXIS   =                    2
   4 | NAXIS1  =                    8
   5 | NAXIS2  =                    4
   6 | PCOUNT  =                    0
   7 | GCOUNT  =                    1
   8 | TFIELDS =                    1
   9 | TTYPE1  = 'DATA    '
  10 | TFORM1  = '1PB(1024)'
  11 | END     
 
 11 header keywords
 
   (1 columns x 4 rows)
 
 Col# Name (Units)       Format
   1 DATA                 1PB(1024) 
 
++++++++++++++++++++++ Error Summary  ++++++++++++++++++++++
 
 HDU#  Name (version)       Type             Warnings  Errors
 1                          Primary Array    0         0     
 2     SCI                  Image Array      0         0     
 3                          Binary Table     0         0     
 
**** Verification found 0 warning(s) and 0 error(s). ****
````


### fitsverify -l on tzim candidate

````text
 
              fitsverify 4.22 (CFITSIO V4.7.0)              
              --------------------------------              
 
 
File: /home/runner/work/_temp/fits-heasarc-work/fixtures/multi_hdu.fits

4 Header-Data Units in this file.
 
=================== HDU 1: Primary Array ===================
 
   1 | SIMPLE  =                    T / conforms to FITS standard
   2 | BITPIX  =                    8 / array data type
   3 | NAXIS   =                    0 / number of array dimensions
   4 | EXTEND  =                    T
   5 | END     
 
 5 header keywords
 
 Null data array; NAXIS = 0 
 
=================== HDU 2: BINARY Table ====================
 
   1 | XTENSION= 'BINTABLE'           / binary table extension
   2 | BITPIX  =                    8 / array data type
   3 | NAXIS   =                    2 / number of array dimensions
   4 | NAXIS1  =                    8 / width of table in bytes
   5 | NAXIS2  =                  128 / number of rows in table
   6 | PCOUNT  =                 6867 / number of group parameters
   7 | GCOUNT  =                    1 / number of groups
   8 | TFIELDS =                    1 / number of fields in each row
   9 | TTYPE1  = 'COMPRESSED_DATA'
  10 | TFORM1  = '1PB(55)'
  11 | ZIMAGE  =                    T / extension contains compressed image
  12 | ZSIMPLE =                    T
  13 | ZBITPIX =                   16 / array data type
  14 | ZNAXIS  =                    2 / number of array dimensions
  15 | ZNAXIS1 =                   64
  16 | ZNAXIS2 =                  128
  17 | ZTILE1  =                   64 / size of tiles to be compressed
  18 | ZTILE2  =                    1 / size of tiles to be compressed
  19 | ZCMPTYPE= 'GZIP_1'             / compression algorithm
  20 | EXTNAME = 'COMPRESSED_IMAGE'   / name of this binary table extension
  21 | END     
 
 21 header keywords
 
 COMPRESSED_IMAGE  (1 columns x 128 rows)
 
 Col# Name (Units)       Format
   1 COMPRESSED_DATA      1PB(55)   
 
=================== HDU 3: BINARY Table ====================
 
   1 | XTENSION= 'BINTABLE'           / binary table extension
   2 | BITPIX  =                    8 / array data type
   3 | NAXIS   =                    2 / number of array dimensions
   4 | NAXIS1  =                    8 / width of table in bytes
   5 | NAXIS2  =                   64 / number of rows in table
   6 | PCOUNT  =                 6440 / number of group parameters
   7 | GCOUNT  =                    1 / number of groups
   8 | TFIELDS =                    1 / number of fields in each row
   9 | TTYPE1  = 'COMPRESSED_DATA'
  10 | TFORM1  = '1PB(106)'
  11 | ZIMAGE  =                    T / extension contains compressed image
  12 | ZTENSION= 'IMAGE'              / Image extension
  13 | ZBITPIX =                  -32 / array data type
  14 | ZNAXIS  =                    2 / number of array dimensions
  15 | ZNAXIS1 =                   32
  16 | ZNAXIS2 =                   64
  17 | ZPCOUNT =                    0 / number of parameters
  18 | ZGCOUNT =                    1 / number of groups
  19 | ZTILE1  =                   32 / size of tiles to be compressed
  20 | ZTILE2  =                    1 / size of tiles to be compressed
  21 | ZCMPTYPE= 'GZIP_2'             / compression algorithm
  22 | ZNAME1  = 'NOISEBIT'           / floating point quantization level
  23 | ZVAL1   =                    0 / floating point quantization level
  24 | EXTNAME = 'SCI     '
  25 | END     
 
 25 header keywords
 
 SCI  (1 columns x 64 rows)
 
 Col# Name (Units)       Format
   1 COMPRESSED_DATA      1PB(106)  
 
=================== HDU 4: BINARY Table ====================
 
   1 | XTENSION= 'BINTABLE'
   2 | BITPIX  =                    8
   3 | NAXIS   =                    2
   4 | NAXIS1  =                    8
   5 | NAXIS2  =                    4
   6 | PCOUNT  =                    0
   7 | GCOUNT  =                    1
   8 | TFIELDS =                    1
   9 | TTYPE1  = 'DATA    '
  10 | TFORM1  = '1PB(1024)'
  11 | END     
 
 11 header keywords
 
   (1 columns x 4 rows)
 
 Col# Name (Units)       Format
   1 DATA                 1PB(1024) 
 
++++++++++++++++++++++ Error Summary  ++++++++++++++++++++++
 
 HDU#  Name (version)       Type             Warnings  Errors
 1                          Primary Array    0         0     
 2     COMPRESSED_IMAGE     Binary Table     0         0     
 3     SCI                  Binary Table     0         0     
 4                          Binary Table     0         0     
 
**** Verification found 0 warning(s) and 0 error(s). ****
````


### funpack -C on tzim candidate

````text
````


## HDRLET preservation-only baseline comparison

Original fitsverify exit 1; candidate exit 1; funpack -C exit not run.
The original's unregistered HDRLET diagnostic is an explicit per-file baseline; candidate diagnostic markers and counts match that baseline: False.
A different candidate error is reported as new; the inherited unregistered-XTENSION error is not a global warning/error allowlist. HDRLET raw bytes must match exactly.

Original HDU layout (byte offsets):
- HDU 0 PRIMARY: start=0, header=2880, data=16384, padded-data=17280, next=20160
- HDU 1 HDRLET: start=20160, header=2880, data=800, padded-data=2880, next=25920
- HDU 2 IMAGE: start=25920, header=2880, data=8192, padded-data=8640, next=37440
- HDU 3 BINTABLE: start=37440, header=2880, data=32, padded-data=2880, next=43200

Candidate HDU layout (byte offsets):
- HDU 0 PRIMARY: start=0, header=2880, data=0, padded-data=0, next=2880
- HDU 1 BINTABLE: start=2880, header=2880, data=7891, padded-data=8640, next=14400
- HDU 2 HDRLET: start=14400, header=2880, data=800, padded-data=2880, next=20160
- HDU 3 BINTABLE: start=20160, header=2880, data=6952, padded-data=8640, next=31680
- HDU 4 BINTABLE: start=31680, header=2880, data=32, padded-data=2880, next=37440

### fitsverify -l on original input

````text
 
              fitsverify 4.22 (CFITSIO V4.7.0)              
              --------------------------------              
 
 
File: /home/runner/work/_temp/fits-heasarc-work/scratch/original-hdrlet_preservation.fits

4 Header-Data Units in this file.
 
=================== HDU 1: Primary Array ===================
 
   1 | SIMPLE  =                    T
   2 | BITPIX  =                   16
   3 | NAXIS   =                    2
   4 | NAXIS1  =                   64
   5 | NAXIS2  =                  128
   6 | EXTEND  =                    T
   7 | END     
 
 7 header keywords
 
 16-bit integer pixels,  2 axes (64 x 128), 
 
=================== HDU 2: Image Exten. ====================
 
*** Error:   Unregistered XTENSION value "HDRLET  ".
 
   1 | XTENSION= 'HDRLET  '
   2 | BITPIX  =                    8
   3 | NAXIS   =                    1
   4 | NAXIS1  =                  800
   5 | PCOUNT  =                    0
   6 | GCOUNT  =                    1
   7 | EXTNAME = 'STWCS   '
   8 | END     
 
 8 header keywords
 
STWCS 8-bit integer pixels,  1 axes (800), 
 
=================== HDU 3: Image Exten. ====================
 
   1 | XTENSION= 'IMAGE   '
   2 | BITPIX  =                  -32
   3 | NAXIS   =                    2
   4 | NAXIS1  =                   32
   5 | NAXIS2  =                   64
   6 | PCOUNT  =                    0
   7 | GCOUNT  =                    1
   8 | EXTNAME = 'SCI     '
   9 | END     
 
 9 header keywords
 
SCI 32-bit floating point pixels,  2 axes (32 x 64), 
 
=================== HDU 4: BINARY Table ====================
 
   1 | XTENSION= 'BINTABLE'
   2 | BITPIX  =                    8
   3 | NAXIS   =                    2
   4 | NAXIS1  =                    8
   5 | NAXIS2  =                    4
   6 | PCOUNT  =                    0
   7 | GCOUNT  =                    1
   8 | TFIELDS =                    1
   9 | TTYPE1  = 'DATA    '
  10 | TFORM1  = '1PB(1024)'
  11 | END     
 
 11 header keywords
 
   (1 columns x 4 rows)
 
 Col# Name (Units)       Format
   1 DATA                 1PB(1024) 
 
++++++++++++++++++++++ Error Summary  ++++++++++++++++++++++
 
 HDU#  Name (version)       Type             Warnings  Errors
 1                          Primary Array    0         0     
 2     STWCS                Image Array      0         1     
 3     SCI                  Image Array      0         0     
 4                          Binary Table     0         0     
 
**** Verification found 0 warning(s) and 1 error(s). ****
````


### fitsverify -l on tzim candidate

````text
 
              fitsverify 4.22 (CFITSIO V4.7.0)              
              --------------------------------              
 
 
File: /home/runner/work/_temp/fits-heasarc-work/fixtures/hdrlet_preservation.fits

2 Header-Data Units in this file.
 
=================== HDU 1: Primary Array ===================
 
   1 | SIMPLE  =                    T / conforms to FITS standard
   2 | BITPIX  =                    8 / array data type
   3 | NAXIS   =                    0 / number of array dimensions
   4 | EXTEND  =                    T
   5 | END     
 
 5 header keywords
 
 Null data array; NAXIS = 0 
 
=================== HDU 2: BINARY Table ====================
 
   1 | XTENSION= 'BINTABLE'           / binary table extension
   2 | BITPIX  =                    8 / array data type
   3 | NAXIS   =                    2 / number of array dimensions
   4 | NAXIS1  =                    8 / width of table in bytes
   5 | NAXIS2  =                  128 / number of rows in table
   6 | PCOUNT  =                 6867 / number of group parameters
   7 | GCOUNT  =                    1 / number of groups
   8 | TFIELDS =                    1 / number of fields in each row
   9 | TTYPE1  = 'COMPRESSED_DATA'
  10 | TFORM1  = '1PB(55)'
  11 | ZIMAGE  =                    T / extension contains compressed image
  12 | ZSIMPLE =                    T
  13 | ZBITPIX =                   16 / array data type
  14 | ZNAXIS  =                    2 / number of array dimensions
  15 | ZNAXIS1 =                   64
  16 | ZNAXIS2 =                  128
  17 | ZTILE1  =                   64 / size of tiles to be compressed
  18 | ZTILE2  =                    1 / size of tiles to be compressed
  19 | ZCMPTYPE= 'GZIP_1'             / compression algorithm
  20 | EXTNAME = 'COMPRESSED_IMAGE'   / name of this binary table extension
  21 | END     
 
 21 header keywords
 
 COMPRESSED_IMAGE  (1 columns x 128 rows)
 
 Col# Name (Units)       Format
   1 COMPRESSED_DATA      1PB(55)   
 
*** Error:   Bad HDU? (from CFITSIO error stack:)
             Character 9 in this keyword is illegal. Hex Value = 1
             ^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C
             Name of keyword no. 37 contains illegal character(s): ^A^B^C^D^A^B^C^D^A^B^C^D^A
             Failed to read the required image extension header keywords.
             Failed to move to HDU number 3 (ffmahd).
             Character 9 in this keyword is illegal. Hex Value = 1
             ^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C^D^A^B^C
             Name of keyword no. 37 contains illegal character(s): ^A^B^C^D^A^B^C^D^A^B^C^D^A
             Failed to read the required image extension header keywords.
             Failed to move to HDU number 3 (ffmahd).
             
++++++++++++++++++++++ Error Summary  ++++++++++++++++++++++
 
 HDU#  Name (version)       Type             Warnings  Errors
 1                          Primary Array    0         0     
 2     COMPRESSED_IMAGE     Binary Table     0         0     
 End-of-file                                 0         1     
 
**** Verification found 0 warning(s) and 1 error(s). ****
````


## zero_width_column.fits — owner-confirmed strict whole-file size result

**Input:** HDU 0: empty primary (BITPIX=8, NAXIS=0); HDU 1: IMAGE extension (BITPIX=16, NAXIS=2, NAXIS1=32, NAXIS2=64, EXTNAME='SCI'); HDU 2: BINTABLE (BITPIX=8, NAXIS=2, NAXIS1=8, NAXIS2=2, TFIELDS=2, TFORM1='0A' [zero-width], TFORM2='8A', PCOUNT=0, GCOUNT=1; 16 data bytes).

Input SHA-256: `57eb1edaadd40271e5e4a75e07c73d38008854fd235ae789fad1b1db4b58b3cf`; output SHA-256: `57eb1edaadd40271e5e4a75e07c73d38008854fd235ae789fad1b1db4b58b3cf`.

**Contract:** §4.12's strict whole-file size gate applies after the per-HDU candidate is formed: if the complete candidate is not strictly smaller, the original remains unchanged and the status is already_optimal. The discarded candidate size is not needed to assert that externally visible contract.

**Observed:** status `already_optimal`, note `None`; bytes 17280 → 17280.

Expected result is unchanged + `already_optimal`: the strict whole-file size gate applies after candidate formation. The discarded candidate size is not needed; no zero-width behavior was expanded.

## Controls: two negative controls plus one positive toolchain sanity check

- `corrupt_tile` (negative): expected intact tile bytes match source; flipped tile does not; record `{"control_type": "negative", "corrupted_at": 7818, "detected": true, "expected": "intact tile bytes match source; flipped tile does not", "fitsverify_exit": 0, "fixture": "int16_primary.fits", "name": "corrupt_tile", "note": "the corrupted tile no longer inflates: Error -3 while decompressing data: invalid bit length repeat", "tile_length": 70, "tile_offset": 0}`.
- `broken_structure` (negative): expected fitsverify exit must be nonzero for NAXIS1/TFORM1 contradiction; record `{"card": "NAXIS1  =                    8 / width of table in bytes", "control_type": "negative", "detected": true, "expected": "fitsverify exit must be nonzero for NAXIS1/TFORM1 contradiction", "fitsverify_exit": 1, "name": "broken_structure", "replacement": "NAXIS1 = 16 (TFORM1 says 8)"}`.
- `toolchain` (positive_sanity): expected fpack -> funpack -C; fitsverify exit 0 and zero literal 80-column header diff; record `{"array_equal": false, "control_type": "positive_sanity", "expected": "fpack -> funpack -C; fitsverify exit 0 and zero literal 80-column header diff", "fitsverify_exit": 0, "fpack_exit": 0, "funpack_exit": 0, "header_diff_count": 9, "name": "toolchain", "ok": false}`.

The corrupted-tile control tests the byte-integrity comparison; fitsverify may accept structurally valid FITS with changed image content. The structural control must make fitsverify nonzero. The positive fpack control is `fpack → funpack -C`: its output must pass fitsverify and its full literal header diff must be empty. The fpack reference is integer-only; float arrays are compared only as original versus `tzim → funpack -C`.

Full control-tool output and round-trip diffs are retained in the checksummed evidence.

